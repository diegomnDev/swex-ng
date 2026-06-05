//! Parity port of SWEX's `profile-export.js` `sortUserData`.
//!
//! Source of truth: `sw-exporter/app/plugins/profile-export.js` (the
//! `ProfileExport` plugin ships with `sortData: true` by default, so the
//! original ALWAYS runs this before writing the profile). swex-ng previously
//! wrote the decrypted response verbatim, which leaked two com2us quirks into
//! the file:
//!
//!   1. A monster's `runes` (and the top-level inventory `runes`) is sometimes
//!      serialized by the game's PHP backend as a JSON **object** keyed by
//!      arbitrary integers (e.g. `"6".."11"`) instead of a JSON **array** —
//!      `json_encode` emits an object whenever the PHP array isn't 0-indexed.
//!      Every downstream tool expects an array. The original fixes this with
//!      `monster.runes = Object.values(monster.runes)`.
//!   2. Ordering: the original sorts `unit_list`, each monster's runes, the
//!      inventory runes, and `rune_craft_item_list` to match the in-game order.
//!
//! Reproducing both makes our output byte-for-byte equivalent to the original
//! Summoners War Exporter (combined with serde_json's `preserve_order`, which
//! keeps the server's original key order instead of alphabetizing it).

use serde_json::Value;
use std::cmp::Ordering;

/// Integer field accessor mirroring the original's numeric comparisons.
fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// com2us (PHP `json_encode`) sometimes serializes a rune list as an *object*
/// keyed by arbitrary integers instead of a JSON array. The original coerces it
/// with `Object.values(...)`, which keeps the values and discards the keys.
/// We then sort by `slot_no`, so the pre-sort iteration order is irrelevant.
fn coerce_to_array(field: &mut Value) {
    if let Value::Object(map) = field {
        let values = std::mem::take(map).into_iter().map(|(_, v)| v).collect();
        *field = Value::Array(values);
    }
}

/// Port of npm `sanitize-filename` (default empty replacement) — how
/// sw-exporter builds the profile name: `sanitize(`${name}-${id}`) + ".json"`.
/// Mirrors its regexes exactly so our filenames match the original.
pub fn sanitize_filename(input: &str) -> String {
    // illegalRe `[\/\?<>\\:\*\|"]` + controlRe `[\x00-\x1f\x80-\x9f]`
    let mut s: String = input
        .chars()
        .filter(|c| !is_illegal(*c) && !is_control_code(*c))
        .collect();
    // reservedRe `^\.+$` — entire string is dots
    if !s.is_empty() && s.chars().all(|c| c == '.') {
        s.clear();
    }
    // windowsReservedRe `^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$` (i)
    if is_windows_reserved(&s) {
        s.clear();
    }
    // windowsTrailingRe `[\. ]+$`
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    // truncate-utf8-bytes to 255 bytes without splitting a codepoint
    if s.len() > 255 {
        let mut end = 255;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

fn is_illegal(c: char) -> bool {
    matches!(c, '/' | '?' | '<' | '>' | '\\' | ':' | '*' | '|' | '"')
}

fn is_control_code(c: char) -> bool {
    let u = c as u32;
    (0x00..=0x1f).contains(&u) || (0x80..=0x9f).contains(&u)
}

fn is_windows_reserved(s: &str) -> bool {
    let base = s.split('.').next().unwrap_or(s).to_ascii_lowercase();
    matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && base.as_bytes()[3].is_ascii_digit())
}

/// Port of `sortUserData(data)`. Mutates `data` in place.
pub fn sort_user_data(data: &mut Value) {
    let Some(obj) = data.as_object_mut() else {
        return;
    };

    // Storage building id (`building_master_id === 25`) — monsters parked in
    // storage sort to the end, exactly like the original.
    let storage_id: Option<i64> =
        obj.get("building_list")
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|b| num(b, "building_master_id") == 25)
                    .map(|b| num(b, "building_id"))
            });

    // Sort monsters and normalize each monster's equipped runes.
    if let Some(units) = obj.get_mut("unit_list").and_then(Value::as_array_mut) {
        for monster in units.iter_mut() {
            if let Some(runes) = monster.get_mut("runes") {
                coerce_to_array(runes);
                if let Value::Array(arr) = runes {
                    arr.sort_by_key(|r| num(r, "slot_no"));
                }
            }
        }
        units.sort_by(|a, b| unit_order(a, b, storage_id));
    }

    // Inventory runes: normalize object -> array, then sort by (set_id, slot_no).
    if let Some(runes) = obj.get_mut("runes") {
        coerce_to_array(runes);
        if let Value::Array(arr) = runes {
            arr.sort_by(|a, b| {
                num(a, "set_id")
                    .cmp(&num(b, "set_id"))
                    .then(num(a, "slot_no").cmp(&num(b, "slot_no")))
            });
        }
    }

    // Rune crafts: sort by (craft_type, craft_item_id).
    if let Some(Value::Array(arr)) = obj.get_mut("rune_craft_item_list") {
        arr.sort_by(|a, b| {
            num(a, "craft_type")
                .cmp(&num(b, "craft_type"))
                .then(num(a, "craft_item_id").cmp(&num(b, "craft_item_id")))
        });
    }
}

/// `unit_list` comparator.
///
/// The original compares two JS arrays of `cmp()` results (each `-1|0|1`) via
/// `x > y` — JS coerces both to comma-joined strings, so the comparison reduces
/// to the **sign of the first non-zero term**. That makes it a plain
/// lexicographic key:
///   `[ storage asc, class desc, unit_level desc, attribute asc, unit_id asc ]`
fn unit_order(a: &Value, b: &Value, storage_id: Option<i64>) -> Ordering {
    let is_storage = |u: &Value| -> i64 {
        match storage_id {
            Some(id) if num(u, "building_id") == id => 1,
            _ => 0,
        }
    };
    is_storage(a)
        .cmp(&is_storage(b)) // storage monsters last
        .then(num(b, "class").cmp(&num(a, "class"))) // -cmp(a,b) => class desc
        .then(num(b, "unit_level").cmp(&num(a, "unit_level"))) // unit_level desc
        .then(num(a, "attribute").cmp(&num(b, "attribute"))) // attribute asc
        .then(num(a, "unit_id").cmp(&num(b, "unit_id"))) // unit_id asc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The tricky case from a real export (unit 5543980210): the game sent
    /// `runes` as an object keyed "6".."11" — the keys are NOT slot_no
    /// (key 6 -> slot 1 ... key 11 -> slot 6). It must become a slot-ordered
    /// array, and must SERIALIZE as a JSON array.
    #[test]
    fn monster_runes_object_becomes_slot_ordered_array() {
        let mut data = json!({
            "building_list": [],
            "unit_list": [{
                "unit_id": 5543980210i64,
                "class": 6, "unit_level": 40, "attribute": 1, "building_id": 0,
                "runes": {
                    "6":  {"rune_id": 20471608130i64, "slot_no": 1, "set_id": 1},
                    "7":  {"rune_id": 58086220034i64, "slot_no": 2, "set_id": 1},
                    "8":  {"rune_id": 17331050404i64, "slot_no": 3, "set_id": 1},
                    "9":  {"rune_id": 59328892657i64, "slot_no": 4, "set_id": 1},
                    "10": {"rune_id": 59275335868i64, "slot_no": 5, "set_id": 1},
                    "11": {"rune_id": 41436781632i64, "slot_no": 6, "set_id": 1}
                }
            }],
            "runes": []
        });

        sort_user_data(&mut data);

        let runes = &data["unit_list"][0]["runes"];
        assert!(runes.is_array(), "runes must be a JSON array, got: {runes}");
        let arr = runes.as_array().unwrap();
        assert_eq!(arr.len(), 6, "all six equipped runes must survive");
        let slots: Vec<i64> = arr.iter().map(|r| r["slot_no"].as_i64().unwrap()).collect();
        assert_eq!(
            slots,
            vec![1, 2, 3, 4, 5, 6],
            "runes must be ordered by slot_no"
        );

        // The actual breakage was the serialized type: it MUST be `[...]`.
        let serialized = serde_json::to_string(runes).unwrap();
        assert!(
            serialized.starts_with('['),
            "serialized runes must be a JSON array, got: {serialized}"
        );
    }

    #[test]
    fn monster_runes_already_array_is_sorted_and_stays_array() {
        let mut data = json!({
            "building_list": [],
            "unit_list": [{
                "unit_id": 1, "class": 6, "unit_level": 40, "attribute": 1, "building_id": 0,
                "runes": [
                    {"rune_id": 3, "slot_no": 3, "set_id": 1},
                    {"rune_id": 1, "slot_no": 1, "set_id": 1},
                    {"rune_id": 2, "slot_no": 2, "set_id": 1}
                ]
            }],
            "runes": []
        });

        sort_user_data(&mut data);

        let arr = data["unit_list"][0]["runes"].as_array().unwrap();
        let slots: Vec<i64> = arr.iter().map(|r| r["slot_no"].as_i64().unwrap()).collect();
        assert_eq!(slots, vec![1, 2, 3]);
    }

    #[test]
    fn inventory_runes_object_becomes_array_sorted_by_set_then_slot() {
        let mut data = json!({
            "building_list": [],
            "unit_list": [],
            "runes": {
                "0": {"rune_id": 1, "set_id": 2, "slot_no": 5},
                "1": {"rune_id": 2, "set_id": 1, "slot_no": 3},
                "2": {"rune_id": 3, "set_id": 1, "slot_no": 1}
            }
        });

        sort_user_data(&mut data);

        let runes = &data["runes"];
        assert!(runes.is_array(), "inventory runes must be an array");
        let keys: Vec<(i64, i64)> = runes
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["set_id"].as_i64().unwrap(),
                    r["slot_no"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(keys, vec![(1, 1), (1, 3), (2, 5)]);
    }

    #[test]
    fn unit_list_sorted_like_ingame() {
        // storage building 99; storage monster must sort last. Non-storage
        // monsters: higher class first, then higher level.
        let mut data = json!({
            "building_list": [{"building_master_id": 25, "building_id": 99}],
            "unit_list": [
                {"unit_id": 1, "class": 5, "unit_level": 40, "attribute": 1, "building_id": 0,  "runes": []},
                {"unit_id": 2, "class": 6, "unit_level": 30, "attribute": 1, "building_id": 0,  "runes": []},
                {"unit_id": 3, "class": 6, "unit_level": 40, "attribute": 1, "building_id": 99, "runes": []},
                {"unit_id": 4, "class": 6, "unit_level": 40, "attribute": 1, "building_id": 0,  "runes": []}
            ],
            "runes": []
        });

        sort_user_data(&mut data);

        let order: Vec<i64> = data["unit_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["unit_id"].as_i64().unwrap())
            .collect();
        // class6/lvl40 (4), class6/lvl30 (2), class5 (1), then storage class6 (3)
        assert_eq!(order, vec![4, 2, 1, 3]);
    }

    #[test]
    fn sanitize_filename_matches_original() {
        // normal name -> untouched
        assert_eq!(sanitize_filename("SnoopCG-6062946"), "SnoopCG-6062946");
        // illegal chars stripped (matches sw-exporter's illegalRe)
        assert_eq!(sanitize_filename("a/b:c*d-1"), "abcd-1");
        // control codes stripped
        assert_eq!(sanitize_filename("na\u{0000}me\u{001f}-1"), "name-1");
        // trailing dots/spaces stripped
        assert_eq!(sanitize_filename("name. "), "name");
        // reserved windows name (whole string) wiped
        assert_eq!(sanitize_filename("CON"), "");
        // but a name merely containing it is fine
        assert_eq!(sanitize_filename("console-1"), "console-1");
        // truncation to 255 bytes
        assert_eq!(sanitize_filename(&"x".repeat(300)).len(), 255);
    }

    #[test]
    fn rune_craft_item_list_sorted() {
        let mut data = json!({
            "building_list": [],
            "unit_list": [],
            "runes": [],
            "rune_craft_item_list": [
                {"craft_type": 2, "craft_item_id": 10},
                {"craft_type": 1, "craft_item_id": 20},
                {"craft_type": 1, "craft_item_id": 5}
            ]
        });

        sort_user_data(&mut data);

        let keys: Vec<(i64, i64)> = data["rune_craft_item_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["craft_type"].as_i64().unwrap(),
                    c["craft_item_id"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(keys, vec![(1, 5), (1, 20), (2, 10)]);
    }
}
