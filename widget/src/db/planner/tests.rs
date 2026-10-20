use super::*;

#[test]
fn loader_requires_complete_rows_positive_quantities_and_valid_metadata() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE items (code INTEGER PRIMARY KEY);
        CREATE TABLE recipes (id INTEGER PRIMARY KEY, source TEXT, result_item INTEGER, result_qty INTEGER, fee INTEGER, learn_fee INTEGER, success_rate INTEGER);
        CREATE TABLE recipe_materials (recipe_id INTEGER, slot INTEGER, item_code INTEGER, qty INTEGER, enhance INTEGER);
        INSERT INTO items VALUES (1),(2),(3),(4),(5),(6),(7),(9),(10),(100);
        INSERT INTO recipes VALUES (1,'alchemy',1,1,10,NULL,NULL), (2,'alchemy',2,1,10,NULL,NULL),
            (3,'alchemy',3,0,10,NULL,NULL), (4,'alchemy',4,1,10,NULL,NULL),
            (5,'alchemy',5,1,10,NULL,NULL), (6,'alchemy',6,1,10,NULL,0),
            (7,'alchemy',7,1,10,NULL,NULL), (8,'alchemy',999,1,10,NULL,NULL),
            (9,'alchemy',9,1,-1,NULL,NULL), (10,'alchemy',10,1,10,-1,NULL);
        INSERT INTO recipe_materials VALUES (1,1,100,2,0),(2,1,999,2,0),
            (3,1,100,2,0),(4,1,100,-2,0),(6,1,100,2,0),(7,1,100,2,-1),
            (8,1,100,2,0),(9,1,100,2,0),(10,1,100,2,0);
    ").unwrap();
    let loaded = PlannerCatalog::load(&conn).unwrap();
    assert_eq!(loaded.recipes.keys().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(loaded.recipes[&1].success, None);
    assert_eq!(loaded.result_code(1), Some(1));
    for id in 2..=10 {
        assert_eq!(loaded.result_code(id), None, "invalid recipe {id}");
    }
}

#[test]
fn duplicate_material_slots_merge_by_enhancement_and_reject_overflow() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE items (code INTEGER PRIMARY KEY);
        CREATE TABLE recipes (id INTEGER PRIMARY KEY, source TEXT, result_item INTEGER, result_qty INTEGER, fee INTEGER, learn_fee INTEGER, success_rate INTEGER);
        CREATE TABLE recipe_materials (recipe_id INTEGER, slot INTEGER, item_code INTEGER, qty INTEGER, enhance INTEGER);
        INSERT INTO items VALUES (1),(2),(100);
        INSERT INTO recipes VALUES (1,'alchemy',1,1,10,NULL,100), (2,'alchemy',2,1,10,NULL,NULL);
        INSERT INTO recipe_materials VALUES (1,1,100,2,0),(1,2,100,3,0),(1,3,100,1,5),
            (2,1,100,9223372036854775807,0),(2,2,100,1,0);
    ").unwrap();
    let loaded = PlannerCatalog::load(&conn).unwrap();
    assert_eq!(loaded.recipes.keys().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(
        loaded.recipes[&1].materials[&ItemKey {
            code: 100,
            enhance: 0
        }],
        5
    );
    assert_eq!(
        loaded.recipes[&1].materials[&ItemKey {
            code: 100,
            enhance: 5
        }],
        1
    );
}
