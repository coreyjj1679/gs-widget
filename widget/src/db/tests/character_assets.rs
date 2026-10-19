use super::*;

fn bundled_root() -> PathBuf {
    find_db().expect("database").parent().unwrap().join("client").canonicalize().expect("bundled client")
}

fn assert_visible_frame(path: &Path, frame: i32) {
    let image = crate::agf::frame_rgba(path, frame).unwrap_or_else(|error| {
        panic!("{} frame {frame}: {error}", path.display())
    });
    assert!(image.width > 0 && image.height > 0);
    assert_eq!(image.rgba.len(), image.width as usize * image.height as usize * 4);
    assert!(image.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0), "{} frame {frame} is blank", path.display());
}

#[test]
fn every_bundled_mount_portrait_and_animation_resolves_and_decodes() {
    let catalog = Catalog::open().expect("catalog");
    let bundled = bundled_root();
    let mounts: Vec<_> = catalog.index.heroes.iter().filter(|hero| hero.rank == 13).collect();
    assert_eq!(mounts.len(), 15);
    for mount in mounts {
        let (path, frame) = catalog.hero_portrait(&mount.code).unwrap_or_else(|| panic!("missing mount portrait {}", mount.code));
        assert!(path.canonicalize().unwrap().starts_with(&bundled), "{} must use shipped artwork", mount.code);
        assert_visible_frame(&path, frame);
        let (path, first) = catalog.hero_battle_sprite(&mount.code).unwrap_or_else(|| panic!("missing mount animation {}", mount.code));
        assert!(path.canonicalize().unwrap().starts_with(&bundled), "{} animation must use shipped artwork", mount.code);
        assert_visible_frame(&path, first);
        let frames = crate::agf::playable_frames(&path).expect("mount animation frames");
        assert!(frames.len() > 1);
        for index in [0, frames.len() / 2, frames.len() - 1] {
            assert_visible_frame(&path, frames[index]);
        }
    }
}

#[test]
fn every_bundled_spirit_has_decodable_animation_frames() {
    let catalog = Catalog::open().expect("catalog");
    let bundled = bundled_root();
    let spirits: Vec<_> = catalog.index.heroes.iter().filter(|hero| hero.rank == 14).collect();
    assert_eq!(spirits.len(), 33);
    for spirit in spirits {
        let (path, first) = catalog.hero_battle_sprite(&spirit.code).unwrap_or_else(|| panic!("missing spirit animation {}", spirit.code));
        assert!(path.canonicalize().unwrap().starts_with(&bundled), "{} must use shipped artwork", spirit.code);
        assert_visible_frame(&path, first);
        let frames = crate::agf::playable_frames(&path).expect("animation frames");
        assert!(frames.len() > 1, "{} needs an animation sequence", spirit.code);
        for index in [0, frames.len() / 2, frames.len() - 1] {
            assert_visible_frame(&path, frames[index]);
        }
    }
}

#[test]
fn animation_sequence_skips_transparent_tail_frames_without_changing_decoding() {
    let catalog = Catalog::open().expect("catalog");
    let (path, _) = catalog.hero_battle_sprite("F16").expect("bundled water spirit");
    let frames = crate::agf::playable_frames(&path).expect("animation frames");
    for frame in 140..144 {
        // The source sheet keeps a positive-size transparent tail. It remains
        // decodable, but must not make the looping animation flash empty.
        let image = crate::agf::frame_rgba(&path, frame).expect("transparent source frame");
        assert!(image.width > 0 && image.height > 0);
        assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0));
        assert!(!frames.contains(&frame));
    }
    assert_visible_frame(&path, 0);
    assert!(frames.contains(&0));
}
