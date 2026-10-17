use super::*;
use serde_json::json;
use std::cell::Cell;

struct Fixture {
    root: tempfile::TempDir,
    next_id: u128,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for lineup in ["jp", "us"] {
            fs::create_dir(root.path().join(lineup)).unwrap();
            put_json(&root.path().join(lineup).join("gamelist.json"), &json!([]));
        }
        fs::write(root.path().join(".chronos-library-v2"), b"2\n").unwrap();
        Self { root, next_id: 1 }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn container(&self, name: &str) {
        let path = self.path().join(name);
        if path.exists() {
            return;
        }
        fs::create_dir(&path).unwrap();
        put_json(
            &path.join("folder.json"),
            &json!({"name":"Folder", "future":true}),
        );
        put_json(&path.join("gamelist.json"), &json!([]));
        self.append(
            path.parent().unwrap(),
            row(path.file_name().unwrap().to_str().unwrap()),
        );
    }

    fn append(&self, parent: &Path, entry: Value) {
        let path = parent.join("gamelist.json");
        let mut rows = read_json(&path).as_array().unwrap().clone();
        rows.push(entry);
        put_json(&path, &Value::Array(rows));
    }

    fn game(&mut self, base: &str, name: &str, tag: &str) -> PathBuf {
        self.container(base);
        let directory = self.path().join(base).join(name);
        fs::create_dir(&directory).unwrap();
        let game = json!({
            "game_id":format!("{:032x}", self.next_id),
            "native_region_tag":tag,
            "display":{"name":"Game", "titlebar":9, "ccolor":3,
                "nativeExtra":{"enabled":true}},
            "rom":{"arch":"tg16", "country":base.split('/').next().unwrap(),
                "rom":"test.pce", "regionTag":tag, "nativeExtra":[1, true, 0.5]},
            "emulator":{"ScreenMode":4, "nativeFlags":[2, 3]},
            "rom_alt":{"regionTagE":format!("{tag}E"), "future":7},
            "future":{"nested":[null, {"keep":"verbatim"}]},
            "cover":"cover.png", "genre":2
        });
        self.next_id += 1;
        put_json(&directory.join("game.json"), &game);
        fs::write(directory.join("test.pce"), vec![0x5a; 8192]).unwrap();
        fs::write(directory.join("cover.png"), b"opaque existing artwork").unwrap();
        self.append(&self.path().join(base), row(name));
        directory
    }

    fn plans(
        &self,
        source: &str,
        name: &str,
        destination: &str,
    ) -> (Vec<GameListEntry>, Vec<GameListEntry>) {
        let source_rows: Vec<GameListEntry> =
            serde_json::from_value(read_json(&self.path().join(source).join("gamelist.json")))
                .unwrap();
        let moved = source_rows
            .iter()
            .find(|r| r.folder == name)
            .unwrap()
            .clone();
        let source = source_rows
            .into_iter()
            .filter(|r| r.folder != name)
            .collect();
        let mut destination: Vec<GameListEntry> = serde_json::from_value(read_json(
            &self.path().join(destination).join("gamelist.json"),
        ))
        .unwrap();
        destination.push(moved);
        (source, destination)
    }
}

fn row(name: &str) -> Value {
    json!({"folder":name, "sor_date":0, "sor_demo":1, "sor_genr":2,
        "sor_name":3, "sor_pnum":4, "futureRow":{"keep":[name, 7]}})
}

fn put_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, directory: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if kind.is_dir() {
                out.insert(relative, b"directory".to_vec());
                collect(root, &path, out);
            } else if kind.is_symlink() {
                out.insert(
                    relative,
                    format!("symlink:{}", fs::read_link(path).unwrap().display()).into_bytes(),
                );
            } else {
                out.insert(relative, fs::read(path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    collect(root, root, &mut out);
    out
}

fn add_saved_data(game: &Path) {
    fs::create_dir(game.join("saves")).unwrap();
    fs::create_dir(game.join("runtime")).unwrap();
    fs::write(game.join("sram.bin"), vec![0xa7; 8448]).unwrap();
    fs::write(game.join("saves/.chronos-sram-schema"), b"2\n").unwrap();
    fs::write(game.join("saves/locks.bin"), [0b1010]).unwrap();
    // Moving an identified game must preserve existing opaque native pairs;
    // it must neither reinterpret payloads nor renumber any of the four slots.
    for slot in 0..4 {
        fs::write(
            game.join(format!("saves/state_{slot}.bin")),
            vec![slot + 1; 257],
        )
        .unwrap();
        fs::write(
            game.join(format!("saves/state_{slot}_meta.bin")),
            format!("metadata-{slot}"),
        )
        .unwrap();
    }
    fs::write(
        game.join("runtime/assets.json"),
        b"{\"revision\":\"immutable-cover\"}",
    )
    .unwrap();
}

#[test]
fn both_catalogues_commit_before_strict_scan_for_root_folder_and_lineup_moves() {
    for (source, destination) in [
        ("jp", "jp/FOLDER_A"),
        ("jp/FOLDER_A", "jp"),
        ("jp/FOLDER_A", "jp/FOLDER_B"),
        ("jp", "us"),
        ("jp/FOLDER_A", "us/FOLDER_B"),
        ("us/FOLDER_A", "jp"),
    ] {
        let mut fixture = Fixture::new();
        let original = fixture.game(source, "GAME001", "MOVING");
        fixture.game(source, "STAY", "STAY");
        fixture.game(destination, "OTHER", "OTHER");
        add_saved_data(&original);
        let saved = snapshot(&original);
        let mut expected_game = read_json(&original.join("game.json"));
        expected_game["rom"]["country"] = json!(destination.split('/').next().unwrap());
        let (mut source_plan, mut destination_plan) = fixture.plans(source, "GAME001", destination);
        source_plan.reverse();
        source_plan[0].sor_genr = 8;
        destination_plan[0].sor_date = 9;
        destination_plan.last_mut().unwrap().sor_name = 17;
        let refresh_count = Cell::new(0);
        let result = perform_with(
            fixture.path(),
            source,
            "GAME001",
            destination,
            &source_plan,
            &destination_plan,
            &m2_publish::save_format::atomic_write,
            &|root| {
                refresh_count.set(refresh_count.get() + 1);
                // This is the strict scanner used beneath font preparation.
                // Calling it before both raw lists commit reproduces the bug.
                let library = m2_publish::load_library(root).map_err(|e| e.to_string())?;
                let mut occurrences = 0;
                for lineup in [&library.jp, &library.us] {
                    for (_, games) in lineup.iter_publish_folders() {
                        for game in games {
                            if !game.rom_path().is_file() {
                                return Err("ROM reference is missing".into());
                            }
                            if game.region_tag == "MOVING" {
                                occurrences += 1;
                            }
                        }
                    }
                }
                if occurrences != 1 {
                    return Err(format!("Found moved game {occurrences} times"));
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(
            result.warning.is_none(),
            "{source} -> {destination}: {:?}",
            result.warning
        );
        assert_eq!(refresh_count.get(), 1);
        assert_eq!(result.library.source_schema, 2);
        assert!(!original.exists());
        let moved = fixture.path().join(destination).join("GAME001");
        assert_eq!(read_json(&moved.join("game.json")), expected_game);
        let after = snapshot(&moved);
        for (path, bytes) in &saved {
            if path != Path::new("game.json") {
                assert_eq!(after.get(path), Some(bytes), "{}", path.display());
            }
        }
        let source_rows = read_json(&fixture.path().join(source).join("gamelist.json"));
        let destination_rows = read_json(&fixture.path().join(destination).join("gamelist.json"));
        assert_eq!(source_rows[0]["sor_genr"], 8);
        assert_eq!(destination_rows[0]["sor_date"], 9);
        let moved_row = destination_rows.as_array().unwrap().last().unwrap();
        assert_eq!(moved_row["sor_name"], 17);
        assert_eq!(moved_row["futureRow"], row("GAME001")["futureRow"]);
        assert_eq!(
            source_rows[0]["futureRow"],
            row(&source_plan[0].folder)["futureRow"]
        );
    }
}

#[test]
fn colliding_directory_name_gets_suffix_without_changing_identity_or_existing_game() {
    let mut fixture = Fixture::new();
    let source = fixture.game("jp", "GAME001", "ORIGINAL");
    let occupied = fixture.game("us", "GAME001", "EXISTING");
    fs::create_dir(fixture.path().join("us/GAME001-2")).unwrap();
    add_saved_data(&source);
    let occupied_before = snapshot(&occupied);
    let source_before = snapshot(&source);
    let identity = read_json(&source.join("game.json"));
    let (source_sort, destination_sort) = fixture.plans("jp", "GAME001", "us");
    let result = perform_with(
        fixture.path(),
        "jp",
        "GAME001",
        "us",
        &source_sort,
        &destination_sort,
        &m2_publish::save_format::atomic_write,
        &|_| Ok(()),
    )
    .unwrap();
    assert_eq!(result.folder_name, "GAME001-3");
    assert_eq!(snapshot(&occupied), occupied_before);
    let moved = fixture.path().join("us/GAME001-3");
    let after = read_json(&moved.join("game.json"));
    assert_eq!(after["game_id"], identity["game_id"]);
    assert_eq!(after["native_region_tag"], "ORIGINAL");
    assert_eq!(after["rom"]["regionTag"], "ORIGINAL");
    assert_eq!(after["display"]["titlebar"], 9);
    let moved_snapshot = snapshot(&moved);
    for (path, bytes) in source_before {
        if path != Path::new("game.json") {
            assert_eq!(moved_snapshot.get(&path), Some(&bytes));
        }
    }
    let rows = read_json(&fixture.path().join("us/gamelist.json"));
    assert_eq!(rows[0]["folder"], "GAME001");
    assert_eq!(rows[1]["folder"], "GAME001-3");
}

#[test]
fn every_write_failure_rolls_back_exact_original_bytes_even_after_replacement() {
    for fail_at in 1..=3 {
        for persisted_before_error in [false, true] {
            let mut fixture = Fixture::new();
            let source = fixture.game("jp/FOLDER_A", "GAME001", "MOVING");
            fixture.game("us/FOLDER_B", "OTHER", "OTHER");
            add_saved_data(&source);
            let before = snapshot(fixture.path());
            let (source_sort, destination_sort) =
                fixture.plans("jp/FOLDER_A", "GAME001", "us/FOLDER_B");
            let writes = Cell::new(0);
            let refreshes = Cell::new(0);
            let error = perform_with(
                fixture.path(),
                "jp/FOLDER_A",
                "GAME001",
                "us/FOLDER_B",
                &source_sort,
                &destination_sort,
                &|path, data| {
                    writes.set(writes.get() + 1);
                    if writes.get() == fail_at {
                        if persisted_before_error {
                            m2_publish::save_format::atomic_write(path, data)?;
                        }
                        return Err("injected durable-write failure".into());
                    }
                    m2_publish::save_format::atomic_write(path, data)
                },
                &|_| {
                    refreshes.set(refreshes.get() + 1);
                    Ok(())
                },
            )
            .unwrap_err();
            assert!(error.contains("rolled back"), "{error}");
            assert_eq!(writes.get(), fail_at);
            assert_eq!(refreshes.get(), 0);
            assert_eq!(
                snapshot(fixture.path()),
                before,
                "write {fail_at}, persisted={persisted_before_error}"
            );
            m2_publish::load_library(fixture.path()).unwrap();
        }
    }
}

#[test]
fn refresh_failure_returns_committed_move_with_warning_and_no_retry_ambiguity() {
    let mut fixture = Fixture::new();
    let source = fixture.game("jp", "GAME001", "MOVING");
    add_saved_data(&source);
    let (source_sort, destination_sort) = fixture.plans("jp", "GAME001", "us");
    let result = perform_with(
        fixture.path(),
        "jp",
        "GAME001",
        "us",
        &source_sort,
        &destination_sort,
        &m2_publish::save_format::atomic_write,
        &|_| Err("font disk full".into()),
    )
    .unwrap();
    assert!(result.warning.unwrap().contains("font disk full"));
    assert!(!source.exists());
    assert!(fixture
        .path()
        .join("us/GAME001/saves/state_3.bin")
        .is_file());
    assert_eq!(
        read_json(&fixture.path().join("jp/gamelist.json")),
        json!([])
    );
    assert_eq!(
        read_json(&fixture.path().join("us/gamelist.json"))[0]["folder"],
        "GAME001"
    );
    m2_publish::load_library(fixture.path()).unwrap();
}

#[test]
fn stale_sort_plans_are_rejected_before_any_write_or_move() {
    for variant in 0..6 {
        let mut fixture = Fixture::new();
        fixture.game("jp", "GAME001", "MOVING");
        fixture.game("jp", "STAY", "STAY");
        fixture.game("jp", "STAY2", "STAY2");
        fixture.game("us", "OTHER", "OTHER");
        let (mut source_sort, mut destination_sort) = fixture.plans("jp", "GAME001", "us");
        match variant {
            0 => {
                source_sort.pop();
            }
            1 => source_sort[0].folder = "UNKNOWN".into(),
            2 => source_sort[1].folder = source_sort[0].folder.clone(),
            3 => {
                destination_sort.pop();
            }
            4 => destination_sort[0].folder = "UNKNOWN".into(),
            _ => destination_sort.swap(0, 1),
        }
        let before = snapshot(fixture.path());
        let writes = Cell::new(0);
        let error = perform_with(
            fixture.path(),
            "jp",
            "GAME001",
            "us",
            &source_sort,
            &destination_sort,
            &|_, _| {
                writes.set(writes.get() + 1);
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("game list"), "{error}");
        assert_eq!(writes.get(), 0);
        assert_eq!(snapshot(fixture.path()), before);
    }
}

#[test]
fn native_tag_or_game_id_collision_rejects_move_without_changing_any_saved_data() {
    for duplicate_id in [false, true] {
        let mut fixture = Fixture::new();
        let source = fixture.game("jp", "GAME001", "MOVING");
        let destination = fixture.game("us", "OTHER", "OTHER");
        add_saved_data(&source);
        let mut other = read_json(&destination.join("game.json"));
        if duplicate_id {
            other["game_id"] = read_json(&source.join("game.json"))["game_id"].clone();
        } else {
            other["native_region_tag"] = json!("MOVING");
        }
        put_json(&destination.join("game.json"), &other);
        let before = snapshot(fixture.path());
        let (source_sort, destination_sort) = fixture.plans("jp", "GAME001", "us");
        let writes = Cell::new(0);
        let error = perform_with(
            fixture.path(),
            "jp",
            "GAME001",
            "us",
            &source_sort,
            &destination_sort,
            &|_, _| {
                writes.set(writes.get() + 1);
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap_err();
        assert!(
            error.contains("conflicts") || error.contains("Duplicate game_id"),
            "{error}"
        );
        assert_eq!(writes.get(), 0);
        assert_eq!(snapshot(fixture.path()), before);
    }
}

#[test]
fn invalid_paths_and_same_container_are_rejected_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.game("jp", "GAME001", "MOVING");
    fixture.game("us", "OTHER", "OTHER");
    let (source_sort, destination_sort) = fixture.plans("jp", "GAME001", "us");
    let before = snapshot(fixture.path());
    for (source, name, destination) in [
        ("../jp", "GAME001", "us"),
        ("jp", "../GAME001", "us"),
        ("jp", "GAME001", "us/../jp"),
        ("jp", "GAME001", "us/"),
        ("jp", "GAME001", "us/./OTHER"),
        ("jp", "GAME001", "us\\OTHER"),
        ("jp", "GAME001", "us/OTHER"),
        ("jp", "GAME001", "jp"),
        ("jp", "", "us"),
        ("jp", ".", "us"),
        ("jp", "..", "us"),
        ("jp", "GAME001\\child", "us"),
        ("jp", "/GAME001", "us"),
    ] {
        let writes = Cell::new(0);
        assert!(perform_with(
            fixture.path(),
            source,
            name,
            destination,
            &source_sort,
            &destination_sort,
            &|_, _| {
                writes.set(writes.get() + 1);
                Ok(())
            },
            &|_| Ok(())
        )
        .is_err());
        assert_eq!(writes.get(), 0);
        assert_eq!(
            snapshot(fixture.path()),
            before,
            "{source}/{name} -> {destination}"
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_catalogue_or_game_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;
    for catalogue in [false, true] {
        let mut fixture = Fixture::new();
        let game = fixture.game("jp", "GAME001", "MOVING");
        fixture.game("us", "OTHER", "OTHER");
        let (source_sort, destination_sort) = fixture.plans("jp", "GAME001", "us");
        if catalogue {
            let index = fixture.path().join("us/gamelist.json");
            fs::rename(&index, fixture.path().join("saved-index.json")).unwrap();
            symlink("../saved-index.json", index).unwrap();
        } else {
            fs::rename(&game, fixture.path().join("saved-game")).unwrap();
            symlink("../saved-game", game).unwrap();
        }
        let before = snapshot(fixture.path());
        let error = perform_with(
            fixture.path(),
            "jp",
            "GAME001",
            "us",
            &source_sort,
            &destination_sort,
            &m2_publish::save_format::atomic_write,
            &|_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("regular"), "{error}");
        assert_eq!(snapshot(fixture.path()), before);
    }
}
