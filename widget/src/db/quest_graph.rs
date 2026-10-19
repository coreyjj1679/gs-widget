//! Dependency relationships come from the imported quest metadata, not display order.
//! Codes are namespaced by source. Only the intro table is known to reference main
//! quests across namespaces; matching an arbitrary code in another source is unsafe.
use super::text::lead3;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::Connection;

use super::quest::quest_source_rank;
use super::text::label;
use super::{Catalog, Hit};

/// Logical-pixel geometry for the native, pannable dependency roadmap.
#[derive(Clone, Default)]
pub struct QuestGraphLayout {
    pub nodes: Vec<QuestGraphNode>,
    pub edges: Vec<QuestGraphEdge>,
    pub segments: Vec<QuestGraphSegment>,
    pub width: f32,
    pub height: f32,
    /// Center of the selected node, suitable for initial viewport centering.
    pub selected_x: f32,
    pub selected_y: f32,
    pub note: String,
}

#[derive(Clone)]
pub struct QuestGraphNode {
    pub hit: Hit,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub level: i32,
    /// 0 other branch, 1 prerequisite, 2 current, 3 follow-up, 4 unresolved.
    pub state: i32,
    pub missing: bool,
    pub cyclic: bool,
}

#[derive(Clone)]
pub struct QuestGraphEdge {
    pub from_key: String,
    pub to_key: String,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub bend_y: f32,
    pub cyclic: bool,
    pub unresolved: bool,
    pub highlighted: bool,
}

/// Canonical routed geometry, including intermediate lanes on long DAG edges.
/// Every segment is horizontal or vertical. The final segment enters downward.
#[derive(Clone)]
pub struct QuestGraphSegment {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub cyclic: bool,
    pub unresolved: bool,
    pub highlighted: bool,
    pub arrow: bool,
}

const NODE_WIDTH: f32 = 232.0;
const NODE_HEIGHT: f32 = 160.0;
const COLUMN_GAP: f32 = 36.0;
const LEVEL_GAP: f32 = 56.0;
const GRAPH_PADDING: f32 = 48.0;

pub(in crate::db) struct QuestDependencyComponent {
    pub canonical_id: i64,
    pub quest_ids: Vec<i64>,
    pub root_ids: Vec<i64>,
    pub edge_count: usize,
    pub unresolved_count: usize,
    pub cyclic: bool,
}

#[derive(Debug)]
struct QuestNode {
    source: String,
    code: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Resolution {
    Found(i64),
    Missing,
    Ambiguous,
}

#[derive(Debug, Eq, PartialEq)]
struct UnresolvedReference {
    code: String,
    following: bool,
    ambiguous: bool,
}

#[derive(Default)]
struct QuestGraph {
    nodes: BTreeMap<i64, QuestNode>,
    parents: BTreeMap<i64, BTreeSet<i64>>,
    children: BTreeMap<i64, BTreeSet<i64>>,
    unresolved: BTreeMap<i64, Vec<UnresolvedReference>>,
    ranks: HashMap<i64, usize>,
}

fn has_reference(code: &str) -> bool {
    !code.trim().is_empty() && code.trim() != "0"
}

fn unique(ids: &[i64]) -> Resolution {
    match ids {
        [] => Resolution::Missing,
        [id] => Resolution::Found(*id),
        _ => Resolution::Ambiguous,
    }
}

fn resolve(codes: &HashMap<(String, String), Vec<i64>>, source: &str, code: &str) -> Resolution {
    let local = codes.get(&(source.to_string(), code.trim().to_string()));
    if let Some(ids) = local {
        return unique(ids);
    }
    // Intro's prerequisite field stores main-quest codes (for example 71562).
    // Letter, mercenary and dispatch codes must remain in their own namespaces.
    if source == "intro" {
        if let Some(ids) = codes.get(&("main".to_string(), code.trim().to_string())) {
            return unique(ids);
        }
    }
    Resolution::Missing
}

pub(in crate::db) fn resolve_quest_id(conn: &Connection, source: &str, code: &str) -> Option<i64> {
    if !has_reference(code) {
        return None;
    }
    let mut stmt = conn
        .prepare(
            "SELECT id, source FROM quests
         WHERE code = ?1 AND (source = ?2 OR (?2 = 'intro' AND source = 'main'))",
        )
        .ok()?;
    let rows = stmt
        .query_map((code.trim(), source), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;
    let mut codes: HashMap<(String, String), Vec<i64>> = HashMap::new();
    for row in rows {
        let (id, namespace) = row.ok()?;
        codes
            .entry((namespace, code.trim().to_string()))
            .or_default()
            .push(id);
    }
    match resolve(&codes, source, code) {
        Resolution::Found(id) => Some(id),
        _ => None,
    }
}

impl QuestGraph {
    fn load(conn: &Connection) -> Result<Self, String> {
        let mut graph = Self::default();
        let mut codes: HashMap<(String, String), Vec<i64>> = HashMap::new();
        let mut references = Vec::new();
        let mut stmt = conn
            .prepare("SELECT id, source, code, prev_code, next_code FROM quests ORDER BY id")
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (id, source, code, previous, next) = row.map_err(|err| err.to_string())?;
            codes
                .entry((source.clone(), code.clone()))
                .or_default()
                .push(id);
            graph.nodes.insert(id, QuestNode { source, code });
            graph.parents.insert(id, BTreeSet::new());
            graph.children.insert(id, BTreeSet::new());
            for (code, following) in [(previous, false), (next, true)] {
                if has_reference(&code) {
                    references.push((id, code, following));
                }
            }
        }
        // Read every requirement together: opening a component never runs a query
        // per edge, and multiple Quest requirements are all retained.
        let mut stmt = conn
            .prepare(
                "SELECT quest_id, code FROM quest_steps
             WHERE lower(role) = 'requirement' AND lower(kind) = 'quest'
             ORDER BY quest_id, code",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (id, code) = row.map_err(|err| err.to_string())?;
            if has_reference(&code) {
                references.push((id, code, false));
            }
        }
        for (id, code, following) in references {
            let Some(node) = graph.nodes.get(&id) else {
                continue;
            };
            match resolve(&codes, &node.source, &code) {
                Resolution::Found(target) => {
                    let (parent, child) = if following {
                        (id, target)
                    } else {
                        (target, id)
                    };
                    graph.children.entry(parent).or_default().insert(child);
                    graph.parents.entry(child).or_default().insert(parent);
                }
                failure => {
                    let issue = UnresolvedReference {
                        code: code.trim().to_string(),
                        following,
                        ambiguous: failure == Resolution::Ambiguous,
                    };
                    let issues = graph.unresolved.entry(id).or_default();
                    if !issues.contains(&issue) {
                        issues.push(issue);
                    }
                }
            }
        }
        let mut ordered: Vec<_> = graph.nodes.keys().copied().collect();
        ordered.sort_by_key(|id| {
            let node = &graph.nodes[id];
            (
                quest_source_rank(&node.source),
                node.code.parse::<i64>().unwrap_or(i64::MAX),
                node.code.clone(),
                *id,
            )
        });
        graph.ranks = ordered
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, rank))
            .collect();
        Ok(graph)
    }

    fn component(&self, start: i64) -> BTreeSet<i64> {
        if !self.nodes.contains_key(&start) {
            return BTreeSet::new();
        }
        let mut found = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(id) = pending.pop() {
            if !found.insert(id) {
                continue;
            }
            pending.extend(self.parents[&id].iter().copied());
            pending.extend(self.children[&id].iter().copied());
        }
        found
    }

    fn reachable(&self, start: i64, edges: &BTreeMap<i64, BTreeSet<i64>>) -> HashSet<i64> {
        let mut found = HashSet::new();
        let mut pending: Vec<_> = edges.get(&start).into_iter().flatten().copied().collect();
        while let Some(id) = pending.pop() {
            if !found.insert(id) {
                continue;
            }
            pending.extend(edges[&id].iter().copied());
        }
        found
    }

    /// Return a deterministic topological order, then any cycle-blocked nodes.
    /// The cyclic set excludes ordinary descendants which are only blocked by a cycle.
    fn ordered_component(&self, component: &BTreeSet<i64>) -> (Vec<i64>, HashSet<i64>) {
        let mut indegree: HashMap<_, _> = component
            .iter()
            .map(|id| (*id, self.parents[id].len()))
            .collect();
        let mut ready: BTreeSet<_> = component
            .iter()
            .filter(|id| indegree[id] == 0)
            .map(|id| (self.ranks[id], *id))
            .collect();
        let mut ordered = Vec::new();
        while let Some((_, id)) = ready.pop_first() {
            ordered.push(id);
            for child in &self.children[&id] {
                let degree = indegree.get_mut(child).expect("child belongs to component");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert((self.ranks[child], *child));
                }
            }
        }
        let mut blocked: Vec<_> = component
            .iter()
            .filter(|id| indegree[id] > 0)
            .copied()
            .collect();
        blocked.sort_by_key(|id| self.ranks[id]);
        let cyclic = blocked
            .iter()
            .filter(|id| self.reachable(**id, &self.children).contains(id))
            .copied()
            .collect();
        ordered.extend(blocked);
        (ordered, cyclic)
    }

    fn reference_code(&self, id: i64, from_source: &str) -> String {
        let node = &self.nodes[&id];
        if node.source == from_source {
            node.code.clone()
        } else {
            format!("{}:{}", node.source, node.code)
        }
    }

    fn sorted_ids(&self, ids: &BTreeSet<i64>) -> Vec<i64> {
        let mut sorted: Vec<_> = ids.iter().copied().collect();
        sorted.sort_by_key(|id| self.ranks[id]);
        sorted
    }
}

#[derive(Clone)]
struct LayoutPoint {
    level: usize,
    width: f32,
    x: f32,
}

impl QuestGraph {
    fn roadmap(&self, selected: i64, lang: &str, hits: Vec<Hit>, note: String) -> QuestGraphLayout {
        let component = self.component(selected);
        if component.is_empty() {
            return QuestGraphLayout {
                note,
                ..QuestGraphLayout::default()
            };
        }
        let ordered_ids = self.sorted_ids(&component);
        let ancestors = self.reachable(selected, &self.parents);
        let descendants = self.reachable(selected, &self.children);
        let mut titles: HashMap<_, _> =
            hits.into_iter().map(|hit| (hit.key.clone(), hit)).collect();

        // Collapse strongly connected groups before assigning depth. Cyclic
        // references remain real edges; they must not force unbounded levels.
        let mut groups: Vec<Vec<i64>> = Vec::new();
        let mut group_of = HashMap::new();
        for id in &ordered_ids {
            if group_of.contains_key(id) {
                continue;
            }
            let before = self.reachable(*id, &self.parents);
            let after = self.reachable(*id, &self.children);
            let group: Vec<_> = ordered_ids
                .iter()
                .filter(|other| **other == *id || (before.contains(other) && after.contains(other)))
                .copied()
                .collect();
            for member in &group {
                group_of.insert(*member, groups.len());
            }
            groups.push(group);
        }
        let mut group_parents = vec![BTreeSet::new(); groups.len()];
        let mut group_children = vec![BTreeSet::new(); groups.len()];
        for parent in &ordered_ids {
            for child in &self.children[parent] {
                let (from, to) = (group_of[parent], group_of[child]);
                if from != to {
                    group_children[from].insert(to);
                    group_parents[to].insert(from);
                }
            }
        }
        let mut indegree: Vec<_> = group_parents.iter().map(BTreeSet::len).collect();
        let mut ready: BTreeSet<_> = (0..groups.len())
            .filter(|group| indegree[*group] == 0)
            .collect();
        let mut group_level = vec![0usize; groups.len()];
        while let Some(group) = ready.pop_first() {
            for child in &group_children[group] {
                group_level[*child] = group_level[*child].max(group_level[group] + 1);
                indegree[*child] -= 1;
                if indegree[*child] == 0 {
                    ready.insert(*child);
                }
            }
        }
        // A missing prerequisite for a root gets its own visible level above it.
        let shift = usize::from(ordered_ids.iter().any(|id| {
            group_level[group_of[id]] == 0
                && self
                    .unresolved
                    .get(id)
                    .is_some_and(|issues| issues.iter().any(|issue| !issue.following))
        }));
        let mut nodes = Vec::new();
        let mut indices = HashMap::new();
        for id in &ordered_ids {
            let node = &self.nodes[id];
            let key = format!("quest:{id}");
            let hit = titles.remove(&key).unwrap_or_else(|| {
                diagram_hit(node.code.clone(), String::new(), key, String::new())
            });
            let group = group_of[id];
            indices.insert(*id, nodes.len());
            nodes.push(QuestGraphNode {
                hit,
                x: 0.0,
                y: 0.0,
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                level: (group_level[group] + shift) as i32,
                state: if *id == selected {
                    2
                } else if ancestors.contains(id) {
                    1
                } else if descendants.contains(id) {
                    3
                } else {
                    0
                },
                missing: false,
                cyclic: groups[group].len() > 1 || self.children[id].contains(id),
            });
        }
        let mut connections = Vec::new();
        for parent in &ordered_ids {
            for child in self.sorted_ids(&self.children[parent]) {
                connections.push((
                    indices[parent],
                    indices[&child],
                    group_of[parent] == group_of[&child],
                ));
            }
        }
        for owner in &ordered_ids {
            for (number, issue) in self.unresolved.get(owner).into_iter().flatten().enumerate() {
                let owner_index = indices[owner];
                let level = nodes[owner_index].level + if issue.following { 1 } else { -1 };
                let heading = if issue.ambiguous {
                    words(lang, "Ambiguous reference", "編號不唯一")
                } else {
                    words(lang, "Missing quest data", "任務資料缺漏")
                };
                let node = &self.nodes[owner];
                let key = format!("missing:{owner}:{number}");
                let hit = diagram_hit(
                    format!("{} {}", words(lang, "Quest", "任務"), issue.code),
                    format!("{} {} · {heading}", label(&node.source, lang), issue.code),
                    key,
                    words(lang, "Unresolved", "未解析參照").to_string(),
                );
                let missing_index = nodes.len();
                nodes.push(QuestGraphNode {
                    hit,
                    x: 0.0,
                    y: 0.0,
                    width: NODE_WIDTH,
                    height: NODE_HEIGHT,
                    level: level.max(0),
                    state: 4,
                    missing: true,
                    cyclic: false,
                });
                connections.push(if issue.following {
                    (owner_index, missing_index, false)
                } else {
                    (missing_index, owner_index, false)
                });
            }
        }

        // Insert invisible routing lanes where a multi-parent edge skips levels.
        // This preserves every edge without drawing a connector through a box.
        let mut points: Vec<_> = nodes
            .iter()
            .map(|node| LayoutPoint {
                level: node.level as usize,
                width: NODE_WIDTH,
                x: 0.0,
            })
            .collect();
        let mut paths = Vec::new();
        for (from, to, _) in &connections {
            let mut path = vec![*from];
            for level in (points[*from].level + 1)..points[*to].level {
                path.push(points.len());
                points.push(LayoutPoint {
                    level,
                    width: 32.0,
                    x: 0.0,
                });
            }
            path.push(*to);
            paths.push(path);
        }
        let level_count = points.iter().map(|point| point.level).max().unwrap_or(0) + 1;
        let mut levels = vec![Vec::new(); level_count];
        for (index, point) in points.iter().enumerate() {
            levels[point.level].push(index);
        }
        let mut above = vec![Vec::new(); points.len()];
        let mut below = vec![Vec::new(); points.len()];
        for path in &paths {
            for pair in path.windows(2) {
                if points[pair[0]].level < points[pair[1]].level {
                    below[pair[0]].push(pair[1]);
                    above[pair[1]].push(pair[0]);
                }
            }
        }
        // Alternating barycenter sweeps keep sibling branches together and limit
        // crossing at merges. Stable tie-breaking keeps navigation from jumping.
        let mut positions = vec![0.0f32; points.len()];
        for level in &levels {
            for (order, index) in level.iter().enumerate() {
                positions[*index] = order as f32 / level.len().max(1) as f32;
            }
        }
        for sweep in 0..6 {
            let forward = sweep % 2 == 0;
            let order: Vec<_> = if forward {
                (0..level_count).collect()
            } else {
                (0..level_count).rev().collect()
            };
            let neighbors = if forward { &above } else { &below };
            for level in order {
                levels[level].sort_by(|a, b| {
                    let center = |index: usize| {
                        if neighbors[index].is_empty() {
                            positions[index]
                        } else {
                            neighbors[index]
                                .iter()
                                .map(|other| positions[*other])
                                .sum::<f32>()
                                / neighbors[index].len() as f32
                        }
                    };
                    center(*a)
                        .total_cmp(&center(*b))
                        .then_with(|| positions[*a].total_cmp(&positions[*b]))
                        .then_with(|| a.cmp(b))
                });
                for (order, index) in levels[level].iter().enumerate() {
                    positions[*index] = order as f32 / levels[level].len().max(1) as f32;
                }
            }
        }
        let widths: Vec<_> = levels
            .iter()
            .map(|level| {
                level.iter().map(|index| points[*index].width).sum::<f32>()
                    + COLUMN_GAP * level.len().saturating_sub(1) as f32
            })
            .collect();
        let body_width = widths.iter().copied().fold(NODE_WIDTH, f32::max);
        for (level_index, level) in levels.iter().enumerate() {
            let mut x = GRAPH_PADDING + (body_width - widths[level_index]) / 2.0;
            for index in level {
                points[*index].x = x;
                x += points[*index].width + COLUMN_GAP;
            }
        }
        for (index, node) in nodes.iter_mut().enumerate() {
            node.x = points[index].x;
            node.y = GRAPH_PADDING + points[index].level as f32 * (NODE_HEIGHT + LEVEL_GAP);
        }
        // Distinct parents need distinct horizontal lanes in the gap. Sharing
        // one midpoint makes overlapping branch spans look like a single bus,
        // falsely connecting children to unrelated parents. A parent's own
        // children still share its lane, including the first leg of long edges.
        let mut lane_sources = vec![BTreeSet::new(); level_count];
        for path in &paths {
            for pair in path.windows(2) {
                if points[pair[0]].level < points[pair[1]].level {
                    lane_sources[points[pair[0]].level].insert(pair[0]);
                }
            }
        }
        let mut bend_lanes = HashMap::new();
        for (level, sources) in lane_sources.iter().enumerate() {
            let mut sources: Vec<_> = sources.iter().copied().collect();
            sources.sort_by(|a, b| {
                (points[*a].x + points[*a].width / 2.0)
                    .total_cmp(&(points[*b].x + points[*b].width / 2.0))
                    .then_with(|| a.cmp(b))
            });
            for (lane, source) in sources.iter().enumerate() {
                let offset = LEVEL_GAP * (lane + 1) as f32 / (sources.len() + 1) as f32;
                bend_lanes.insert(
                    *source,
                    GRAPH_PADDING + level as f32 * (NODE_HEIGHT + LEVEL_GAP) + NODE_HEIGHT + offset,
                );
            }
        }
        let mut edges = Vec::new();
        let mut segments = Vec::new();
        for ((from, to, cyclic), path) in connections.iter().zip(paths.iter()) {
            let source = &nodes[*from];
            let target = &nodes[*to];
            let unresolved = source.missing || target.missing;
            let highlighted =
                !unresolved && matches!(source.state, 1 | 2) && matches!(target.state, 1 | 2);
            let (x1, y1, x2, y2, bend_y) = if *cyclic {
                // Cyclic groups share a level. Route over their top edges rather
                // than crossing node interiors; self-loops use distinct ports.
                (
                    source.x + NODE_WIDTH * if from == to { 0.7 } else { 0.5 },
                    source.y,
                    target.x + NODE_WIDTH * if from == to { 0.3 } else { 0.5 },
                    target.y,
                    source.y - LEVEL_GAP / 2.0,
                )
            } else {
                (
                    source.x + NODE_WIDTH / 2.0,
                    source.y + NODE_HEIGHT,
                    target.x + NODE_WIDTH / 2.0,
                    target.y,
                    bend_lanes[from],
                )
            };
            edges.push(QuestGraphEdge {
                from_key: source.hit.key.clone(),
                to_key: target.hit.key.clone(),
                x1,
                y1,
                x2,
                y2,
                bend_y,
                cyclic: *cyclic,
                unresolved,
                highlighted,
            });
            let mut route = vec![(x1, y1)];
            if *cyclic {
                route.extend([(x1, bend_y), (x2, bend_y), (x2, y2)]);
            } else {
                let mut previous = (x1, y1);
                for (step, index) in path.iter().enumerate().skip(1) {
                    let last = step + 1 == path.len();
                    let point = &points[*index];
                    let next = if last {
                        (x2, y2)
                    } else {
                        (
                            point.x + point.width / 2.0,
                            GRAPH_PADDING
                                + point.level as f32 * (NODE_HEIGHT + LEVEL_GAP)
                                + NODE_HEIGHT / 2.0,
                        )
                    };
                    let between = bend_lanes[&path[step - 1]];
                    route.extend([(previous.0, between), (next.0, between), next]);
                    previous = next;
                }
            }
            let mut routed: Vec<_> = route
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .map(|pair| QuestGraphSegment {
                    x1: pair[0].0,
                    y1: pair[0].1,
                    x2: pair[1].0,
                    y2: pair[1].1,
                    cyclic: *cyclic,
                    unresolved,
                    highlighted,
                    arrow: false,
                })
                .collect();
            if let Some(last) = routed.last_mut() {
                last.arrow = true;
            }
            segments.extend(routed);
        }
        let selected_node = &nodes[indices[&selected]];
        QuestGraphLayout {
            width: body_width + GRAPH_PADDING * 2.0,
            height: GRAPH_PADDING * 2.0
                + level_count as f32 * NODE_HEIGHT
                + level_count.saturating_sub(1) as f32 * LEVEL_GAP,
            selected_x: selected_node.x + NODE_WIDTH / 2.0,
            selected_y: selected_node.y + NODE_HEIGHT / 2.0,
            nodes,
            edges,
            segments,
            note,
        }
    }
}

fn diagram_hit(title: String, subtitle: String, key: String, aside: String) -> Hit {
    Hit {
        title,
        subtitle,
        key,
        aside,
        icon: None,
        solo: false,
        badge: None,
        chip: String::new(),
    }
}

fn words<'a>(lang: &str, english: &'a str, chinese: &'a str) -> &'a str {
    super::ui_text_ref(lang, chinese, english)
}

impl Catalog {
    /// Load the graph once for all browse groups; never rebuild it per chain.
    pub(in crate::db) fn quest_dependency_components(&self) -> Vec<QuestDependencyComponent> {
        let Ok(graph) = QuestGraph::load(&self.conn) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut components = Vec::new();
        for id in graph.nodes.keys() {
            if seen.contains(id) {
                continue;
            }
            let members = graph.component(*id);
            seen.extend(members.iter().copied());
            let mut root_ids: Vec<_> = members
                .iter()
                .filter(|id| graph.parents[id].is_empty())
                .copied()
                .collect();
            root_ids.sort_unstable();
            let canonical_id = root_ids.first().copied().unwrap_or(*id);
            let cyclic = !graph.ordered_component(&members).1.is_empty();
            components.push(QuestDependencyComponent {
                canonical_id,
                edge_count: members.iter().map(|id| graph.children[id].len()).sum(),
                unresolved_count: members
                    .iter()
                    .filter_map(|id| graph.unresolved.get(id))
                    .map(Vec::len)
                    .sum(),
                root_ids,
                quest_ids: graph.sorted_ids(&members),
                cyclic,
            });
        }
        components.sort_by_key(|component| component.canonical_id);
        components
    }

    /// Include the selected quest, all prerequisites and follow-ups, and sibling
    /// branches. Each row declares its own direct parents so list order cannot be
    /// mistaken for a single mandatory sequence through every branch.
    pub fn quest_dependency_rows(&self, id: i64, lang: &str) -> (Vec<Hit>, String) {
        let Ok(graph) = QuestGraph::load(&self.conn) else {
            return (
                Vec::new(),
                words(
                    lang,
                    "Quest dependencies could not be loaded.",
                    "無法載入任務依賴資料。",
                )
                .to_string(),
            );
        };
        self.quest_dependency_rows_with_graph(&graph, id, lang)
    }

    fn quest_dependency_rows_with_graph(
        &self,
        graph: &QuestGraph,
        id: i64,
        lang: &str,
    ) -> (Vec<Hit>, String) {
        let component = graph.component(id);
        if component.is_empty() {
            return (
                Vec::new(),
                words(lang, "Quest not found.", "找不到此任務。").to_string(),
            );
        }
        let (ordered, cyclic) = graph.ordered_component(&component);
        let ancestors = graph.reachable(id, &graph.parents);
        let descendants = graph.reachable(id, &graph.children);
        let titles: HashMap<_, _> = self
            .index
            .quests
            .iter()
            .map(|quest| (quest.id, quest))
            .collect();
        let mut hits = Vec::new();
        for node_id in ordered {
            let node = &graph.nodes[&node_id];
            let relation = if node_id == id {
                words(lang, "Current quest", "目前任務")
            } else if ancestors.contains(&node_id) && descendants.contains(&node_id) {
                words(lang, "Dependency cycle", "循環依賴")
            } else if ancestors.contains(&node_id) {
                words(lang, "Prerequisite", "前置任務")
            } else if descendants.contains(&node_id) {
                words(lang, "Follow-up", "後續任務")
            } else {
                words(lang, "Other branch", "其他分支")
            };
            let mut details = vec![format!("{} {}", label(&node.source, lang), node.code)];
            let parents = graph
                .sorted_ids(&graph.parents[&node_id])
                .into_iter()
                .map(|parent| graph.reference_code(parent, &node.source))
                .collect::<Vec<_>>();
            if !parents.is_empty() {
                details.push(format!(
                    "{} {}",
                    words(lang, "Requires:", "前置："),
                    parents.join(", ")
                ));
            }
            for issue in graph.unresolved.get(&node_id).into_iter().flatten() {
                let heading = match (issue.following, issue.ambiguous) {
                    (false, false) => words(lang, "Unresolved prerequisite:", "未解析前置："),
                    (true, false) => words(lang, "Unresolved follow-up:", "未解析後續："),
                    (false, true) => words(lang, "Ambiguous prerequisite:", "前置編號不唯一："),
                    (true, true) => words(lang, "Ambiguous follow-up:", "後續編號不唯一："),
                };
                details.push(format!("{heading} {}", issue.code));
            }
            if cyclic.contains(&node_id) {
                details.push(words(lang, "Cycle detected", "偵測到循環依賴").to_string());
            }
            let title = titles
                .get(&node_id)
                .map(|quest| lead3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko))
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| node.code.clone());
            hits.push(Hit {
                title,
                subtitle: details.join(" · "),
                key: format!("quest:{node_id}"),
                icon: None,
                solo: false,
                badge: None,
                aside: relation.to_string(),
                chip: String::new(),
            });
        }
        let edge_count: usize = component.iter().map(|id| graph.children[id].len()).sum();
        let unresolved_count: usize = component
            .iter()
            .filter_map(|id| graph.unresolved.get(id))
            .map(Vec::len)
            .sum();
        let mut summary = if component.len() == 1 && edge_count == 0 {
            words(
                lang,
                "No connected prerequisite or follow-up is recorded.",
                "沒有可連接的前置或後續任務紀錄。",
            )
            .to_string()
        } else {
            match super::primary_lang(lang) {
                "ko" => format!("관련 퀘스트 {}개 · 의존 관계 {edge_count}개. 다른 분기가 현재 퀘스트의 필수 선행 조건인 것은 아닙니다.", component.len()),
                "zh" => format!("{} 個相關任務 · {edge_count} 個依賴。其他分支不一定是目前任務的必要前置。", component.len()),
                _ => format!("{} related quests · {edge_count} dependencies. Other branches are not necessarily prerequisites for the current quest.", component.len()),
            }
        };
        if unresolved_count > 0 {
            summary.push_str(&match super::primary_lang(lang) {
                "ko" => format!(" 참조 {unresolved_count}개를 확인할 수 없어 의존 관계가 불완전할 수 있습니다."),
                "zh" => format!(" {unresolved_count} 個參照未能解析，依賴鏈可能不完整。"),
                _ => format!(" {unresolved_count} unresolved reference(s); the chain may be incomplete."),
            });
        }
        if !cyclic.is_empty() {
            summary.push_str(&match super::primary_lang(lang) {
                "ko" => format!(" 퀘스트 {}개에 순환 의존 관계가 있어 선행 조건 순서대로 정렬할 수 없습니다.", cyclic.len()),
                "zh" => format!(" {} 個任務涉及循環依賴，受影響的任務無法按前置排序。", cyclic.len()),
                _ => format!(" {} quest(s) in a dependency cycle; affected quests cannot be ordered by prerequisite.", cyclic.len()),
            });
        }
        (hits, summary)
    }

    pub fn quest_dependency_graph(&self, id: i64, lang: &str) -> QuestGraphLayout {
        let Ok(graph) = QuestGraph::load(&self.conn) else {
            return QuestGraphLayout {
                note: words(
                    lang,
                    "Quest dependencies could not be loaded.",
                    "無法載入任務依賴資料。",
                )
                .to_string(),
                ..QuestGraphLayout::default()
            };
        };
        let (hits, note) = self.quest_dependency_rows_with_graph(&graph, id, lang);
        graph.roadmap(id, lang, hits, note)
    }

    pub(in crate::db) fn quest_immediate_dependencies(&self, id: i64) -> (Vec<i64>, Vec<i64>) {
        let Ok(graph) = QuestGraph::load(&self.conn) else {
            return (Vec::new(), Vec::new());
        };
        let Some(parents) = graph.parents.get(&id) else {
            return (Vec::new(), Vec::new());
        };
        (
            graph.sorted_ids(parents),
            graph.sorted_ids(&graph.children[&id]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(
        quests: &[(i64, &str, &str, &str, &str)],
        requirements: &[(i64, &str)],
    ) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE quests (id INTEGER PRIMARY KEY, source TEXT, code TEXT, prev_code TEXT, next_code TEXT);
             CREATE TABLE quest_steps (quest_id INTEGER, role TEXT, kind TEXT, code TEXT);",
        ).unwrap();
        for (id, source, code, previous, next) in quests {
            conn.execute(
                "INSERT INTO quests VALUES (?1, ?2, ?3, ?4, ?5)",
                (id, source, code, previous, next),
            )
            .unwrap();
        }
        for (id, code) in requirements {
            conn.execute(
                "INSERT INTO quest_steps VALUES (?1, 'requirement', 'Quest', ?2)",
                (id, code),
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn korean_graph_labels_and_dynamic_diagnostics_follow_primary_language() {
        let catalog = Catalog::open().expect("catalog");
        let conn = fixture(
            &[(90001, "main", "90001", "90002", ""),
              (90002, "main", "90002", "90001", ""),
              (90003, "main", "90003", "90002", "")],
            &[(90003, "99999")],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        let (rows, note) = catalog.quest_dependency_rows_with_graph(&graph, 90003, "ko");
        assert_eq!(rows.iter().find(|row| row.key == "quest:90003").unwrap().aside, "현재 퀘스트");
        assert!(note.contains("관련 퀘스트 3개"));
        assert!(note.contains("참조 1개"));
        assert!(note.contains("순환 의존 관계"));
        assert!(!note.contains("related quests") && !note.contains("個相關任務"));
        let (_, bilingual_note) = catalog.quest_dependency_rows_with_graph(&graph, 90003, "ko-en");
        assert_eq!(note, bilingual_note);
    }

    #[test]
    fn branching_component_keeps_multiple_prerequisites_and_siblings() {
        let conn = fixture(
            &[
                (40, "main", "4", "2", ""),
                (30, "main", "3", "1", "4"),
                (20, "main", "2", "1", "4"),
                (10, "main", "1", "", "2"),
                (50, "main", "5", "", ""),
            ],
            &[(40, "3"), (40, "2")],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        let component = graph.component(20);
        assert_eq!(component, BTreeSet::from([10, 20, 30, 40]));
        let (ordered, cyclic) = graph.ordered_component(&component);
        assert_eq!(ordered, vec![10, 20, 30, 40]);
        assert!(cyclic.is_empty());
        assert_eq!(graph.parents[&40], BTreeSet::from([20, 30]));
        assert_eq!(graph.reachable(20, &graph.parents), HashSet::from([10]));
        assert_eq!(graph.reachable(20, &graph.children), HashSet::from([40]));
        assert!(
            !graph.reachable(20, &graph.parents).contains(&30),
            "sibling is not a prerequisite"
        );
        assert_eq!(
            graph.children.values().map(BTreeSet::len).sum::<usize>(),
            4,
            "duplicate metadata edges are deduplicated"
        );
    }

    #[test]
    fn cross_source_resolution_is_limited_to_intro_and_unambiguous() {
        let conn = fixture(
            &[
                (1, "main", "71562", "", ""),
                (2, "intro", "3", "71562", ""),
                (3, "letter", "3", "71562", ""),
                (4, "mercenary", "71562", "", ""),
            ],
            &[],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        assert_eq!(graph.parents[&2], BTreeSet::from([1]));
        assert!(
            graph.parents[&3].is_empty(),
            "a coincidental code match must not create a letter dependency"
        );
        assert_eq!(graph.unresolved[&3].len(), 1);
        assert_eq!(resolve_quest_id(&conn, "intro", "71562"), Some(1));
        assert_eq!(resolve_quest_id(&conn, "letter", "71562"), None);
        assert_eq!(graph.reference_code(1, "intro"), "main:71562");
        conn.execute("INSERT INTO quests VALUES (5, 'main', '71562', '', '')", [])
            .unwrap();
        assert_eq!(resolve_quest_id(&conn, "intro", "71562"), None);
        assert!(QuestGraph::load(&conn).unwrap().unresolved[&2][0].ambiguous);
    }

    #[test]
    fn missing_and_ambiguous_references_remain_reportable() {
        let conn = fixture(
            &[
                (1, "main", "1", "999", "888"),
                (2, "main", "2", "", ""),
                (3, "main", "2", "", ""),
            ],
            &[(1, "999"), (1, "2")],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        assert_eq!(graph.component(1), BTreeSet::from([1]));
        assert_eq!(
            graph.unresolved[&1].len(),
            3,
            "the same missing prerequisite in two fields counts once"
        );
        assert!(graph.unresolved[&1]
            .iter()
            .any(|issue| issue.code == "2" && issue.ambiguous));
        assert!(graph.unresolved[&1]
            .iter()
            .any(|issue| issue.code == "888" && issue.following));
        assert_eq!(resolve_quest_id(&conn, "main", "2"), None);
        assert!(graph.component(999).is_empty());
    }

    #[test]
    fn cycles_terminate_and_do_not_mislabel_blocked_descendants() {
        let conn = fixture(
            &[
                (1, "main", "1", "2", ""),
                (2, "main", "2", "1", ""),
                (3, "main", "3", "2", ""),
                (4, "main", "4", "4", ""),
            ],
            &[],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        let (ordered, cyclic) = graph.ordered_component(&graph.component(1));
        assert_eq!(ordered, vec![1, 2, 3]);
        assert_eq!(cyclic, HashSet::from([1, 2]));
        assert_eq!(
            graph.ordered_component(&graph.component(4)),
            (vec![4], HashSet::from([4]))
        );
    }

    #[test]
    fn bundled_intro_and_missing_letter_requirements_are_visible() {
        let catalog = Catalog::open().expect("bundled database");
        let intro = catalog.quest_id_by_code("intro", "3").unwrap();
        let main = catalog.quest_id_by_code("main", "71562").unwrap();
        let (rows, _) = catalog.quest_dependency_rows(intro, "en-zh");
        assert!(rows
            .iter()
            .any(|row| row.key == format!("quest:{main}") && row.aside == "Prerequisite"));
        assert!(rows
            .iter()
            .any(|row| row.key == format!("quest:{intro}") && row.subtitle.contains("main:71562")));
        let letter = catalog.quest_id_by_code("letter", "30201").unwrap();
        let (rows, summary) = catalog.quest_dependency_rows(letter, "zh-en");
        assert!(summary.contains("依賴鏈可能不完整"));
        assert!(rows.iter().any(|row| row.key == format!("quest:{letter}")
            && row.subtitle.contains("未解析前置： 50110")));
    }

    fn assert_clean_geometry(layout: &QuestGraphLayout) {
        for (index, node) in layout.nodes.iter().enumerate() {
            assert!(node.x >= 0.0 && node.y >= 0.0);
            assert!(node.x + node.width <= layout.width);
            assert!(node.y + node.height <= layout.height);
            for other in layout.nodes.iter().skip(index + 1) {
                assert!(
                    node.x + node.width <= other.x
                        || other.x + other.width <= node.x
                        || node.y + node.height <= other.y
                        || other.y + other.height <= node.y,
                    "{} and {} overlap",
                    node.hit.key,
                    other.hit.key
                );
            }
        }
        assert_eq!(
            layout
                .segments
                .iter()
                .filter(|segment| segment.arrow)
                .count(),
            layout.edges.len()
        );
        for segment in &layout.segments {
            assert!(
                segment.x1 == segment.x2 || segment.y1 == segment.y2,
                "connectors must be orthogonal"
            );
            assert!(
                !segment.arrow || segment.y2 > segment.y1,
                "arrow must enter downward"
            );
            for node in &layout.nodes {
                let crosses = if segment.x1 == segment.x2 {
                    segment.x1 > node.x
                        && segment.x1 < node.x + node.width
                        && segment.y1.min(segment.y2) < node.y + node.height
                        && segment.y1.max(segment.y2) > node.y
                } else {
                    segment.y1 > node.y
                        && segment.y1 < node.y + node.height
                        && segment.x1.min(segment.x2) < node.x + node.width
                        && segment.x1.max(segment.x2) > node.x
                };
                assert!(!crosses, "connector crosses {}", node.hit.key);
            }
        }
    }

    #[test]
    fn roadmap_preserves_merges_and_routes_long_edges_around_nodes() {
        let conn = fixture(
            &[
                (1, "main", "1", "", ""),
                (2, "main", "2", "1", ""),
                (3, "main", "3", "2", ""),
                (4, "main", "4", "1", ""),
                (5, "main", "5", "3", ""),
            ],
            &[(5, "1"), (5, "4")],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        let layout = graph.roadmap(3, "en", Vec::new(), String::new());
        assert_eq!(layout.nodes.len(), 5);
        assert_eq!(layout.edges.len(), 6);
        assert_eq!(
            layout
                .edges
                .iter()
                .filter(|edge| edge.to_key == "quest:5")
                .count(),
            3
        );
        assert!(layout.edges.iter().all(|edge| edge.y2 > edge.y1));
        assert_clean_geometry(&layout);
        let other = graph.roadmap(4, "zh", Vec::new(), String::new());
        let positions = |map: &QuestGraphLayout| {
            map.nodes
                .iter()
                .map(|node| (node.hit.key.clone(), node.x, node.y))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            positions(&layout),
            positions(&other),
            "selection and language must not move the graph"
        );
        assert_eq!(
            layout
                .nodes
                .iter()
                .find(|node| node.state == 2)
                .unwrap()
                .hit
                .key,
            "quest:3"
        );
        assert_eq!(
            other
                .nodes
                .iter()
                .find(|node| node.state == 2)
                .unwrap()
                .hit
                .key,
            "quest:4"
        );
    }

    #[test]
    fn roadmap_draws_missing_references_and_cycles_without_inventing_quests() {
        let conn = fixture(
            &[
                (1, "main", "1", "2", ""),
                (2, "main", "2", "1", ""),
                (3, "main", "3", "2", "888"),
                (4, "main", "4", "4", ""),
            ],
            &[(1, "999")],
        );
        let graph = QuestGraph::load(&conn).unwrap();
        let layout = graph.roadmap(1, "en", Vec::new(), String::new());
        assert_eq!(layout.nodes.len(), 5);
        assert_eq!(layout.nodes.iter().filter(|node| node.missing).count(), 2);
        assert_eq!(layout.nodes.iter().filter(|node| node.cyclic).count(), 2);
        assert_eq!(layout.edges.iter().filter(|edge| edge.cyclic).count(), 2);
        assert_eq!(
            layout.edges.iter().filter(|edge| edge.unresolved).count(),
            2
        );
        assert!(layout
            .nodes
            .iter()
            .filter(|node| node.missing)
            .all(|node| node.state == 4 && node.hit.key.starts_with("missing:")));
        assert_clean_geometry(&layout);
        let self_loop = graph.roadmap(4, "en", Vec::new(), String::new());
        assert_eq!(self_loop.nodes.len(), 1);
        assert_eq!(self_loop.edges.len(), 1);
        assert!(self_loop.edges[0].cyclic);
        assert_ne!(self_loop.edges[0].x1, self_loop.edges[0].x2);
        assert_clean_geometry(&self_loop);
    }

    #[test]
    fn overlapping_unrelated_branches_do_not_share_a_horizontal_bus() {
        let catalog = Catalog::open().expect("bundled database");
        let selected = catalog.quest_id_by_code("main", "72114").unwrap();
        let layout = catalog.quest_dependency_graph(selected, "en");
        let mut overlapping_sources = 0;
        for (index, edge) in layout.edges.iter().enumerate() {
            if edge.cyclic {
                continue;
            }
            for other in layout.edges.iter().skip(index + 1) {
                if other.cyclic || edge.from_key == other.from_key || edge.y1 != other.y1 {
                    continue;
                }
                let overlap = edge.x1.min(edge.x2).max(other.x1.min(other.x2))
                    < edge.x1.max(edge.x2).min(other.x1.max(other.x2));
                if overlap {
                    overlapping_sources += 1;
                    assert_ne!(
                        edge.bend_y, other.bend_y,
                        "unrelated parents {} and {} must not appear connected",
                        edge.from_key, other.from_key
                    );
                }
            }
            // These bundled edges all connect adjacent levels. Verify that the
            // rendered geometry, not only the edge metadata, uses the lane.
            if edge.x1 != edge.x2 {
                assert!(layout
                    .segments
                    .iter()
                    .any(|segment| segment.y1 == edge.bend_y
                        && segment.y2 == edge.bend_y
                        && segment.x1 == edge.x1
                        && segment.x2 == edge.x2));
            }
        }
        assert!(
            overlapping_sources > 0,
            "regression must exercise overlapping branch spans"
        );
        let fanout: Vec<_> = layout
            .edges
            .iter()
            .filter(|edge| edge.from_key == format!("quest:{selected}"))
            .collect();
        assert_eq!(fanout.len(), 5);
        assert!(
            fanout.iter().all(|edge| edge.bend_y == fanout[0].bend_y),
            "siblings retain their parent's shared bus"
        );
        assert_clean_geometry(&layout);
    }

    #[test]
    fn bundled_roadmaps_are_bounded_and_show_cross_source_edges() {
        let catalog = Catalog::open().expect("bundled database");
        let graph = QuestGraph::load(&catalog.conn).unwrap();
        let intro = catalog.quest_id_by_code("intro", "3").unwrap();
        let main = catalog.quest_id_by_code("main", "71562").unwrap();
        let layout = catalog.quest_dependency_graph(intro, "en");
        assert!(layout
            .edges
            .iter()
            .any(|edge| edge.from_key == format!("quest:{main}")
                && edge.to_key == format!("quest:{intro}")));
        assert!(layout
            .nodes
            .iter()
            .any(|node| node.state == 2 && node.hit.key == format!("quest:{intro}")));
        let mut seen = HashSet::new();
        let mut widest = (0.0f32, 0usize);
        let mut tallest = (0.0f32, 0usize);
        for id in graph.nodes.keys() {
            if seen.contains(id) {
                continue;
            }
            let component = graph.component(*id);
            seen.extend(component.iter().copied());
            let map = graph.roadmap(*id, "en", Vec::new(), String::new());
            assert_clean_geometry(&map);
            if map.width > widest.0 {
                widest = (map.width, component.len());
            }
            if map.height > tallest.0 {
                tallest = (map.height, component.len());
            }
        }
        println!(
            "Roadmap bounds: widest {}px ({} quests), tallest {}px ({} quests)",
            widest.0, widest.1, tallest.0, tallest.1
        );
    }
}
