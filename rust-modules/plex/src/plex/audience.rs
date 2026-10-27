//! Per-install, fail-closed household media audience. Curated IDs come from the
//! agent-authored Markdown catalog; this layer never infers suitability from titles.
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, sync::OnceLock};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    audience: String,
    machine_id: String,
    rating_keys: BTreeSet<String>,
    section_keys: BTreeSet<String>,
    part_ids: BTreeSet<String>,
    #[serde(default)]
    stream_ids: BTreeSet<String>,
}

pub(super) struct Policy(Option<Snapshot>);
impl Policy {
    fn kids(bytes: &[u8]) -> Self {
        let snapshot = (bytes.len() <= 65536).then(|| serde_json::from_slice::<Snapshot>(bytes).ok()).flatten()
            .filter(|s| s.version == 1 && s.audience == "kids" && !s.machine_id.is_empty()
                && [&s.rating_keys, &s.section_keys, &s.part_ids, &s.stream_ids].iter()
                    .all(|set| set.iter().all(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()))));
        Self(snapshot)
    }
    fn matches(&self, machine: &str) -> bool { self.0.as_ref().is_some_and(|s| s.machine_id == machine) }
    fn allows_path(&self, machine: &str, raw: &str) -> bool {
        if !self.matches(machine) { return false; }
        let s = self.0.as_ref().unwrap();
        let Some(path) = decoded(raw) else { return false };
        if path.contains("/../") || path.contains("/./") || path.contains('\\') || path.contains('\0') { return false; }
        for (prefix, allowed) in [("/library/metadata/", &s.rating_keys), ("/library/parts/", &s.part_ids),
            ("/library/streams/", &s.stream_ids), ("/library/sections/", &s.section_keys),
            ("/hubs/sections/", &s.section_keys)] {
            for (tail, _) in path.match_indices(prefix) {
                let tail = &path[tail + prefix.len()..];
                let ids = tail.split(['/', '?', '&', '#']).next().unwrap_or("");
                if ids.is_empty() || !ids.split(',').all(|id| allowed.contains(if prefix == "/library/streams/" { id.split('.').next().unwrap_or("") } else { id })) { return false; }
            }
        }
        if path.contains("/library/sections/") && ["/composite", "/thumb", "/art"].iter().any(|suffix| path.contains(suffix)) { return false; }
        if path.contains("/photo/:/transcode") && !path.contains("/library/metadata/") { return false; }
        let endpoint = path.split('?').next().unwrap_or("");
        if !["/", "/identity", "/library/sections", "/library/all", "/hubs", "/hubs/search", "/library/search", "/playQueues"]
            .contains(&endpoint) && !["/library/metadata/", "/library/parts/", "/library/streams/",
                "/library/sections/", "/hubs/", "/playQueues/", "/video/:/transcode/universal/",
                "/photo/:/transcode", "/status/sessions/close", "/actions/removeFromContinueWatching", "/:/"].iter().any(|prefix| endpoint.starts_with(prefix)) { return false; }
        for pair in path.split('?').skip(1).flat_map(|query| query.split('&')) {
            if let Some((key, value)) = pair.split_once('=') {
                if ["ratingKey", "key"].contains(&key) && value.bytes().all(|c| c.is_ascii_digit())
                    && !s.rating_keys.contains(value) { return false; }
            }
        }
        // A transcoder start/decision must name an explicitly allowed metadata item.
        if endpoint.contains("/transcode/universal/") && !endpoint.ends_with("/stop") && !endpoint.ends_with("/ping")
            && !path.contains("/library/metadata/") { return false; }
        true
    }
    fn filter(&self, machine: &str, path: &str, value: &mut Value) {
        let snapshot = self.0.as_ref().filter(|_| self.matches(machine));
        scrub(value, snapshot, path.split('?').next() == Some("/library/sections"));
    }
}

fn decoded(raw: &str) -> Option<String> {
    let mut value = raw.to_owned();
    for _ in 0..4 {
        if !value.contains('%') { return Some(value); }
        let mut out = Vec::new(); let bytes = value.as_bytes(); let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'%' {
                let pair = bytes.get(at + 1..at + 3)?;
                let hex = |c: u8| (c as char).to_digit(16).map(|n| n as u8);
                out.push(hex(pair[0])? * 16 + hex(pair[1])?); at += 3;
            } else { out.push(bytes[at]); at += 1; }
        }
        value = String::from_utf8(out).ok()?;
    }
    (!value.contains('%')).then_some(value)
}

fn allowed_item(value: &Value, snapshot: Option<&Snapshot>) -> bool {
    value.get("ratingKey").and_then(Value::as_str)
        .is_some_and(|key| snapshot.is_some_and(|s| s.rating_keys.contains(key)))
}
fn scrub(value: &mut Value, snapshot: Option<&Snapshot>, sections: bool) {
    match value {
        Value::Object(object) => {
            let counted = ["Metadata", "SearchResult", "Hub", "Directory"].into_iter()
                .find_map(|key| object.get(key).and_then(Value::as_array).map(|items| (key, items.len())));
            let number = |key: &str| object.get(key).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
            let complete = number("offset").unwrap_or(0) == 0
                && !object.get("more").and_then(Value::as_bool).unwrap_or(false)
                && counted.is_some_and(|(_, count)| number("totalSize").is_none_or(|total| total <= count as i64));
            for (key, child) in object.iter_mut() {
                if key == "Metadata" {
                    match child {
                        Value::Array(items) => items.retain(|item| allowed_item(item, snapshot)),
                        _ => { if !allowed_item(child, snapshot) { *child = Value::Null; } }
                    }
                } else if key == "Directory" {
                    match child {
                        Value::Array(items) if sections => {
                            items.retain(|item| item.get("key").and_then(Value::as_str)
                                .is_some_and(|id| snapshot.is_some_and(|s| s.section_keys.contains(id))));
                            for item in items { if let Some(item) = item.as_object_mut() {
                                item.insert("title".into(), Value::String("Cinema Kids".into()));
                                for art in ["thumb", "art", "composite"] { item.remove(art); }
                            } }
                        }
                        Value::Array(items) => items.clear(),
                        _ => *child = Value::Null,
                    }
                }
                scrub(child, snapshot, sections && key == "MediaContainer");
            }
            for key in ["Hub", "SearchResult"] {
                if let Some(Value::Array(items)) = object.get_mut(key) {
                    items.retain(|item| item.get("Metadata").is_some_and(|m| match m {
                        Value::Array(a) => !a.is_empty(), Value::Object(_) => true, _ => false }));
                    if key == "Hub" { for item in items { if let Some(item) = item.as_object_mut() {
                        item.insert("title".into(), Value::String("Cinema Kids".into()));
                        for art in ["thumb", "art", "composite"] { item.remove(art); }
                        // This PMS labels personal-video shelves `clip` even when every
                        // approved item's actual metadata is a movie. The Home projector
                        // excludes clip shelves, so describe the retained movies truthfully.
                        if item.get("Metadata").and_then(Value::as_array).is_some_and(|items|
                            !items.is_empty() && items.iter().all(|m| m.get("type").and_then(Value::as_str) == Some("movie"))) {
                            item.insert("type".into(), Value::String("movie".into()));
                        }
                    } } }
                }
            }
            if let Some((key, _)) = counted {
                let visible = object.get(key).and_then(Value::as_array).map_or(0, Vec::len);
                object.insert("size".into(), Value::from(visible));
                if complete {
                    object.insert("totalSize".into(), Value::from(visible));
                    if object.contains_key("more") { object.insert("more".into(), Value::Bool(false)); }
                } else {
                    // The server's total includes unapproved items. A partial window
                    // cannot establish the filtered global total; never report the raw one.
                    object.remove("totalSize");
                }
            }
        }
        Value::Array(items) => { for item in items { scrub(item, snapshot, sections); } }
        _ => {}
    }
}

pub fn is_kids_install() -> bool { plx_base::paths::app_id().ends_with(".nightly") }
fn policy() -> Option<&'static Policy> {
    static POLICY: OnceLock<Option<Policy>> = OnceLock::new();
    POLICY.get_or_init(|| {
        if !is_kids_install() { return None; }
        use std::io::Read;
        let path = format!("/media/internal/.{}-audience.json", plx_base::paths::app_id());
        let mut bytes = Vec::new();
        if let Ok(file) = std::fs::File::open(path) { let _ = file.take(65537).read_to_end(&mut bytes); }
        Some(Policy::kids(&bytes))
    }).as_ref()
}
pub fn local_profile_ready(machine: &str) -> bool { policy().is_none_or(|policy| policy.matches(machine)) }
pub(super) fn allows_path(machine: &str, path: &str) -> bool {
    policy().is_none_or(|policy| policy.allows_path(machine, path))
}
pub(super) fn allows_art(machine: &str, path: &str) -> bool {
    policy().is_none_or(|policy| policy.allows_path(machine, path)
        && decoded(path).is_some_and(|path| path.contains("/library/metadata/")))
}
pub(super) fn envelope(machine: &str, path: &str, bytes: &[u8]) -> Result<super::models::Envelope, serde_json::Error> {
    let Some(policy) = policy() else { return serde_json::from_slice(bytes) };
    let mut value: Value = serde_json::from_slice(bytes)?;
    policy.filter(machine, path, &mut value);
    serde_json::from_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy { Policy::kids(br#"{"version":1,"audience":"kids","machineId":"household","ratingKeys":["10","11"],"sectionKeys":["1"],"partIds":["20","21"],"streamIds":[]}"#) }
    #[test]
    fn kids_default_unknown_and_other_server_are_denied() {
        assert!(!Policy::kids(b"").allows_path("household", "/library/metadata/10"));
        assert!(!policy().allows_path("another-server", "/library/metadata/10"));
        for path in ["/library/metadata/99", "/library/metadata/10,99", "/library/parts/99/file.mkv", "/library/sections/99/all",
            "/library/collections/99/children", "/:/timeline?ratingKey=99",
            "/library/sections/1/composite/123", "/photo/:/transcode?url=%2Flibrary%2Fsections%2F1%2Fcomposite%2F123",
            "/video/:/transcode/universal/start.mkv?path=%2Flibrary%2Fmetadata%2F99"] {
            assert!(!policy().allows_path("household", path), "{path}");
        }
        for path in ["/library/metadata/10", "/library/parts/20/file.mkv", "/video/:/transcode/universal/ping?session=fixture",
            "/video/:/transcode/universal/stop?session=fixture", "/hubs/continueWatching/items", "/video/:/transcode/universal/start.mkv?path=%2Flibrary%2Fmetadata%2F11"] {
            assert!(policy().allows_path("household", path), "{path}");
        }
    }
    #[test]
    fn sections_are_curated_and_unknown_catalogs_are_empty() {
        let mut value = serde_json::json!({"MediaContainer":{"Directory":[{"key":"1","title":"Movies"},{"key":"9","title":"Adult"}]}});
        policy().filter("household", "/library/sections", &mut value);
        assert_eq!(value["MediaContainer"]["Directory"].as_array().unwrap().len(), 1);
        let mut unknown = serde_json::json!({"MediaContainer":{"Metadata":[{"ratingKey":"10","title":"Bambi"}]}});
        Policy::kids(b"{}").filter("household", "/library/metadata/10", &mut unknown);
        assert!(unknown["MediaContainer"]["Metadata"].as_array().unwrap().is_empty());
    }
    #[test]
    fn actual_pms_clip_shelf_with_curated_movies_is_a_visible_movie_shelf() {
        let mut value = serde_json::json!({"MediaContainer":{"size":1,"Hub":[{"type":"clip","hubIdentifier":"home.videos.recent","size":4,"more":false,"Metadata":[
            {"type":"movie","ratingKey":"99","title":"Obsession"},
            {"type":"movie","ratingKey":"10","title":"Bambi"},
            {"type":"movie","ratingKey":"11","title":"Toy Story 5"},
            {"type":"movie","ratingKey":"98","title":"Obsession"}]}]}});
        let before: super::super::models::Envelope = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(before.media_container.hub[0].kind, "clip", "actual upstream shape is skipped by Home's listable guard");
        policy().filter("household", "/hubs", &mut value);
        let after: super::super::models::Envelope = serde_json::from_value(value).unwrap();
        let shelf = &after.media_container.hub[0];
        assert_eq!(shelf.kind, "movie"); assert_eq!(shelf.size, 2); assert_eq!(shelf.total_size, 2);
        assert!(!shelf.more);
        assert_eq!(shelf.metadata.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), ["Bambi", "Toy Story 5"]);
    }
    #[test]
    fn complete_counts_are_filtered_and_partial_global_totals_are_unknown() {
        for partial in [false, true] {
            let mut value=serde_json::json!({"MediaContainer":{"size":2,"offset":if partial {50} else {0},"totalSize":if partial {100} else {2},"Metadata":[{"ratingKey":"10"},{"ratingKey":"99"}]}});
            policy().filter("household", "/library/sections/1/all", &mut value);
            assert_eq!(value["MediaContainer"]["size"], 1);
            if partial { assert!(value["MediaContainer"].get("totalSize").is_none()); }
            else { assert_eq!(value["MediaContainer"]["totalSize"], 1); }
        }
    }
    #[test]
    fn browse_search_hubs_detail_and_nested_extras_share_one_allowlist() {
        let mut value = serde_json::json!({"MediaContainer":{"Metadata":[{"ratingKey":"10","title":"Bambi","Extras":{"Metadata":[{"ratingKey":"99","title":"Adult extra"}]}},{"ratingKey":"99","title":"Obsession"}],"Hub":[{"Metadata":[{"ratingKey":"99","title":"The Mentalist"}]},{"Metadata":[{"ratingKey":"11","title":"Toy Story 5"}]}],"SearchResult":[{"Metadata":{"ratingKey":"99","title":"Obsession"}},{"Metadata":{"ratingKey":"10","title":"Bambi"}}]}});
        let parents: super::super::models::Envelope = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parents.media_container.metadata.len(), 2, "ordinary Parents DTO parsing stays unrestricted");
        policy().filter("household", "/hubs/search", &mut value);
        let text=value.to_string(); assert!(!text.contains("Obsession") && !text.contains("Mentalist") && !text.contains("Adult extra"));
        assert!(text.contains("Bambi") && text.contains("Toy Story 5"));
        let kids: super::super::models::Envelope = serde_json::from_value(value).unwrap();
        assert_eq!(kids.media_container.metadata.len(), 1);
        assert_eq!(kids.media_container.hub.len(), 1);
        assert_eq!(kids.media_container.search_result.len(), 1);
    }
}
