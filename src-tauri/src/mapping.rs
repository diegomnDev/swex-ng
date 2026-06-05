//! Game data + helper logic ported from SWEX's `app/mapping.js`.
//!
//! The data tables (monster names, rune main/sub stat ranges, sets, artifacts,
//! dungeons...) are embedded from `mapping.json`. The helper *functions* that
//! mapping.js carried (getMonsterName, getRuneEfficiency, getRuneEffect,
//! isAncient) are reimplemented here in Rust — these are the bits relevant to a
//! rune optimizer.
//!
//! NOTE: `getArtifactEffect` sub-stats in the original were stored as JS
//! closures (`artifact.effectTypes.sub[type](value)`), which do not survive the
//! JSON export. Porting them needs the effect-type table rebuilt by hand — left
//! as a TODO so nothing here is invented.

use once_cell::sync::Lazy;
use serde_json::Value;

static MAP: Lazy<Value> = Lazy::new(|| {
    serde_json::from_str(include_str!("../mapping.json")).expect("embedded mapping.json is valid")
});

/// Raw embedded mapping table — exposed for the optimizer / future commands.
#[allow(dead_code)]
pub fn raw() -> &'static Value {
    &MAP
}

/// Port of mapping.js getMonsterName(id).
pub fn get_monster_name(id: i64) -> String {
    if id == 0 {
        return "Unknown Monster".into();
    }
    let names = &MAP["monster"]["names"];
    if let Some(n) = names.get(id.to_string()).and_then(|v| v.as_str()) {
        return n.to_string();
    }
    let s = id.to_string();
    let family: String = s.chars().take(3).collect();
    if let Some(n) = names.get(&family).and_then(|v| v.as_str()) {
        let attr_idx = s.chars().last().unwrap_or('0').to_string();
        let attr = MAP["monster"]["attributes"]
            .get(&attr_idx)
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        return format!("{n} ({attr})");
    }
    "Unknown Monster".into()
}

/// Port of mapping.js isAncient for a rune (class > 10).
pub fn is_ancient_rune(class: i64) -> bool {
    class > 10
}

/// Port of mapping.js getRuneEffect([type, value, ..]).
/// Public helper for the rune optimizer; not called by the proxy itself.
#[allow(dead_code)]
pub fn get_rune_effect(eff: &[Value]) -> String {
    let t = eff.first().and_then(|v| v.as_i64()).unwrap_or(0);
    let v = eff.get(1).and_then(|x| x.as_i64()).unwrap_or(0);
    match t {
        1 => format!("HP +{v}"),
        2 => format!("HP {v}%"),
        3 => format!("ATK +{v}"),
        4 => format!("ATK {v}%"),
        5 => format!("DEF +{v}"),
        6 => format!("DEF {v}%"),
        8 => format!("SPD +{v}"),
        9 => format!("CRI Rate {v}%"),
        10 => format!("CRI Dmg {v}%"),
        11 => format!("Resistance {v}%"),
        12 => format!("Accuracy {v}%"),
        _ => String::new(),
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct RuneEfficiency {
    pub current: f64,
    pub max: f64,
}

// In mapping.json the per-grade `max` table is a JSON *object* keyed by the
// grade as a string ("1".."6"), exactly like mapping.js (`mainstat[t].max[6]`).
// It must be indexed by string key — integer indexing returns JSON null.
fn substat_max6(stat_id: i64) -> f64 {
    MAP["rune"]["substat"][stat_id.to_string()]["max"]["6"]
        .as_f64()
        .unwrap_or(1.0)
}

/// Port of mapping.js getRuneEfficiency(rune). `rune` is the in-profile object.
pub fn get_rune_efficiency(rune: &Value) -> Option<RuneEfficiency> {
    let class = rune["class"].as_i64().unwrap_or(0);
    // mapping.js: max[isAncient ? class-10 : class] — grade is the *key*, 1..6.
    let grade = if is_ancient_rune(class) {
        class - 10
    } else {
        class
    };

    let pri_type = rune["pri_eff"][0].as_i64()?;
    let mainstat = &MAP["rune"]["mainstat"][pri_type.to_string()]["max"];
    let mut ratio = mainstat[grade.to_string()].as_f64()? / mainstat["6"].as_f64()?;

    if let Some(subs) = rune["sec_eff"].as_array() {
        for stat in subs {
            let sid = stat[0].as_i64().unwrap_or(0);
            let base = stat[1].as_f64().unwrap_or(0.0);
            let grind = stat.get(3).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let value = if grind > 0.0 { base + grind } else { base };
            ratio += value / substat_max6(sid);
        }
    }
    if let Some(pre) = rune["prefix_eff"].as_array() {
        let pt = pre.first().and_then(|v| v.as_i64()).unwrap_or(0);
        if pt > 0 {
            let pv = pre.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
            ratio += pv / substat_max6(pt);
        }
    }

    let current = ratio / 2.8 * 100.0;
    let upgrade = rune["upgrade_curr"].as_f64().unwrap_or(0.0);
    let remaining = ((12.0 - upgrade) / 3.0).ceil().max(0.0);
    let max = current + (remaining * 0.2) / 2.8 * 100.0;
    Some(RuneEfficiency { current, max })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round2(x: f64) -> f64 {
        (x * 100.0).round() / 100.0
    }

    // Expected values computed from the ORIGINAL app/mapping.js getRuneEfficiency,
    // so this asserts parity with sw-exporter, not my own arithmetic. Guards the
    // object-vs-array `max` indexing bug from regressing.
    #[test]
    fn rune_efficiency_matches_original() {
        let rune = json!({
            "class": 6,
            "pri_eff": [4, 63],
            "sec_eff": [[8, 30, 0, 0], [1, 1875, 0, 0]],
            "prefix_eff": [0, 0],
            "upgrade_curr": 12
        });
        let e = get_rune_efficiency(&rune).expect("efficiency should compute");
        assert_eq!(round2(e.current), 107.14);
        assert_eq!(round2(e.max), 107.14);
    }

    #[test]
    fn monster_name_matches_original() {
        assert_eq!(get_monster_name(15105), "Devilmon"); // direct id hit
        assert_eq!(get_monster_name(14102), "Phantom Thief (Fire)"); // family + attr
        assert_eq!(get_monster_name(0), "Unknown Monster");
    }
}
