use super::*;
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

struct Example {
    dir: TempDir,
    project: Project,
    piece: SceneSettings,
    source: PathBuf,
}

impl Example {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let source = dir.path().join(format!("{id}.mp4"));
        fs::write(&source, b"first source").unwrap();
        fs::write(dir.path().join("ffmpeg"), b"version one").unwrap();
        let mut project = Project::default();
        project.assets.push(
            serde_json::from_value(json!({
                "id": id, "name": "video", "extension": "mp4", "kind": "video",
                "durationMs": 1000, "hasAudio": true
            }))
            .unwrap(),
        );
        let piece = serde_json::from_value(json!({
            "text": "", "caption": "", "durationMs": 1000, "visual": {"assetId":id, "inMs":0}, "audio":null
        })).unwrap();
        Self {
            dir,
            project,
            piece,
            source,
        }
    }

    fn cache(&self) -> PartCache {
        PartCache::open(
            &self.dir.path().join("cache"),
            &self.dir.path().join("ffmpeg"),
            None,
        )
        .unwrap()
    }

    fn key(&self, cache: &mut PartCache, ass: Option<(&str, u64)>, encoder: &str) -> String {
        cache
            .key(
                &self.project,
                self.dir.path(),
                &self.piece,
                SceneStyle::default(),
                ass,
                encoder,
            )
            .unwrap()
    }
}

#[test]
fn keys_track_render_inputs_across_exports_and_ignore_editor_notes() {
    let mut e = Example::new();
    let mut cache = e.cache();
    let original = e.key(&mut cache, None, "cpu");
    e.piece.notes = "editor note".into();
    e.piece.name = "rename".into();
    assert_eq!(original, e.key(&mut e.cache(), None, "cpu"));
    assert_ne!(original, e.key(&mut cache, None, "gpu"));
    e.piece.visual.as_mut().unwrap().muted = true;
    assert_ne!(original, e.key(&mut cache, None, "cpu"));
    e.piece.visual.as_mut().unwrap().muted = false;
    let a = e.key(&mut cache, Some(("font and text", 0)), "cpu");
    assert_ne!(original, a);
    assert_ne!(a, e.key(&mut cache, Some(("changed text", 0)), "cpu"));
    assert_ne!(a, e.key(&mut cache, Some(("font and text", 1000)), "cpu"));

    let old_time = fs::metadata(&e.source).unwrap().modified().unwrap();
    fs::write(&e.source, b"other source").unwrap(); // same byte length and mtime
    File::options()
        .write(true)
        .open(&e.source)
        .unwrap()
        .set_modified(old_time)
        .unwrap();
    assert_ne!(original, e.key(&mut e.cache(), None, "cpu"));
    fs::remove_file(&e.source).unwrap();
    assert!(
        cache
            .key(
                &e.project,
                e.dir.path(),
                &e.piece,
                SceneStyle::default(),
                None,
                "cpu"
            )
            .is_none()
    );
}

#[test]
fn cache_hit_corruption_repair_and_a_changed_source_during_render() {
    let e = Example::new();
    let cache = &mut e.cache();
    let key = e.key(cache, None, "cpu");
    let part = e.dir.path().join("rendered.mp4");
    let staged = e.dir.path().join("restored.mp4");
    fs::write(&part, b"complete output").unwrap();
    assert!(!cache.restore(&key, &staged));
    cache.publish(&key, &part);
    assert!(cache.restore(&key, &staged));
    assert_eq!(fs::read(&staged).unwrap(), b"complete output");
    let entry = cache.entry(&key);
    let mut bytes = fs::read(&entry).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&entry, bytes).unwrap();
    assert!(!cache.restore(&key, &staged));
    assert_eq!(fs::read(&staged).unwrap(), b"complete output");
    cache.publish(&key, &part);
    assert!(cache.restore(&key, &staged));
    fs::remove_file(&entry).unwrap();
    fs::write(&e.source, b"different source version").unwrap();
    cache.publish(&key, &part);
    assert!(!entry.exists());
}

#[test]
fn simultaneous_exports_publish_whole_records_and_budget_only_removes_cache_entries() {
    let e = Example::new();
    let cache = &mut e.cache();
    let key = e.key(cache, None, "cpu");
    let first = e.dir.path().join("first");
    let second = e.dir.path().join("second");
    fs::write(&first, vec![1; 90_000]).unwrap();
    fs::write(&second, vec![2; 120_000]).unwrap();
    let shared = &*cache;
    std::thread::scope(|scope| {
        scope.spawn(|| shared.publish(&key, &first));
        scope.spawn(|| shared.publish(&key, &second));
    });
    let result = e.dir.path().join("restored");
    assert!(cache.restore(&key, &result));
    let bytes = fs::read(result).unwrap();
    assert!(bytes == vec![1; 90_000] || bytes == vec![2; 120_000]);
    let unrelated = cache.root.join("mine.part");
    fs::write(&unrelated, b"mine").unwrap();
    prune(&cache.root, 16).unwrap();
    assert!(!cache.entry(&key).exists());
    assert!(unrelated.exists());
}
