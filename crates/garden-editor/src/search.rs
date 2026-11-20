//! Searching the catalog, and remembering recent picks.

use crate::catalog::{CatalogEntry, EntryKind, Group};
use crate::strings::{EN, category_label};

/// Lowercase, accents removed, anything but letters and digits as single spaces.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'ç' | 'č' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'ý' | 'ÿ' => "y",
            'ß' => "ss",
            c if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(plain);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How well `entry` matches the (normalised) query words; 0 = not at all.
fn score(entry: &CatalogEntry, query: &str, words: &[&str]) -> u32 {
    let name = normalize(&entry.name);
    let name_words: Vec<&str> = name.split(' ').collect();
    let aliases: Vec<String> = entry.aliases.iter().map(|a| normalize(a)).collect();
    let alias_words: Vec<&str> = aliases.iter().flat_map(|a| a.split(' ')).collect();
    let mut kind_words = vec![normalize(entry.group.label())];
    if let EntryKind::Plant(p) = &entry.kind {
        kind_words.push(normalize(category_label(&EN, p.category)));
    }
    let kind_words: Vec<&str> = kind_words.iter().flat_map(|k| k.split(' ')).collect();
    let all = |pool: &[&str], test: &dyn Fn(&str, &str) -> bool| {
        words.iter().all(|w| pool.iter().any(|p| test(p, w)))
    };
    let equal = |p: &str, w: &str| p == w;
    let prefix = |p: &str, w: &str| p.starts_with(w);
    let with_aliases: Vec<&str> = name_words.iter().chain(&alias_words).copied().collect();
    let with_kinds: Vec<&str> = with_aliases.iter().chain(&kind_words).copied().collect();
    if name == query {
        120
    } else if all(&name_words, &equal) {
        100
    } else if name.starts_with(query) {
        90
    } else if all(&name_words, &prefix) {
        80
    } else if all(&with_aliases, &prefix) {
        60
    } else if all(&with_kinds, &prefix) {
        40
    } else if words
        .iter()
        .all(|w| name.contains(w) || aliases.iter().any(|a| a.contains(w)))
    {
        20
    } else {
        0
    }
}

/// Indices into `entries` matching `query` (within `group` if given), best
/// first; ties keep catalog order. An empty query lists everything.
pub fn search(entries: &[CatalogEntry], query: &str, group: Option<Group>) -> Vec<usize> {
    let query = normalize(query);
    let words: Vec<&str> = query.split(' ').filter(|w| !w.is_empty()).collect();
    let in_group = |e: &CatalogEntry| group.is_none_or(|g| e.group == g);
    if words.is_empty() {
        return (0..entries.len())
            .filter(|&i| in_group(&entries[i]))
            .collect();
    }
    let mut scored: Vec<(u32, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| in_group(e))
        .map(|(i, e)| (score(e, &query, &words), i))
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Recent picks with `id` moved to the front, no duplicates, at most `max`.
pub fn remember(recent: &[String], id: &str, max: usize) -> Vec<String> {
    std::iter::once(id.to_string())
        .chain(recent.iter().filter(|r| r.as_str() != id).cloned())
        .take(max)
        .collect()
}

/// Indices of the entries for `recent` ids, in that order; unknown ids are skipped.
pub fn recent_entries(entries: &[CatalogEntry], recent: &[String]) -> Vec<usize> {
    recent
        .iter()
        .filter_map(|id| entries.iter().position(|e| &e.id == id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Group, catalog};
    use crate::strings::EN;

    fn names(q: &str) -> Vec<String> {
        let c = catalog();
        search(&c, q, None)
            .into_iter()
            .map(|i| c[i].name.clone())
            .collect()
    }

    #[test]
    fn names_and_word_starts_rank_first() {
        assert_eq!(names("lav")[0], "Lavender");
        assert_eq!(names("rose")[0], "Shrub Rose");
        assert!(names("rose").contains(&"Climbing Rose".to_string()));
        assert!(names("rose").contains(&"Rosemary".to_string()));
    }

    #[test]
    fn aliases_find_other_names() {
        assert!(names("maple").contains(&"Japanese Maple".to_string()));
        assert!(names("boxwood").contains(&"Box Ball".to_string()));
        assert!(names("pond").contains(&EN.feat_water.to_string()));
    }

    #[test]
    fn categories_and_groups() {
        let c = catalog();
        let features = search(&c, "", Some(Group::Features));
        assert_eq!(features.len(), 14);
        assert!(features.iter().all(|&i| c[i].group == Group::Features));
        assert!(names("shrub").len() >= 5);
        let trees = search(&c, "lav", Some(Group::Trees));
        assert!(trees.is_empty());
    }

    #[test]
    fn odd_input_is_harmless() {
        assert_eq!(names("").len(), catalog().len());
        assert_eq!(names("   ").len(), catalog().len());
        assert_eq!(names("  Box ")[0], "Box Ball");
        assert_eq!(names("%").len(), catalog().len()); // only punctuation = empty query
        assert!(names(&"z".repeat(200)).is_empty());
        assert_eq!(names("é"), names("e")); // accents ignored
        assert_eq!(names("japanese maple")[0], "Japanese Maple");
        assert!(names("maple japanese").contains(&"Japanese Maple".to_string()));
    }

    #[test]
    fn recent_picks_are_newest_first_without_duplicates() {
        let r = remember(&["a".into(), "b".into()], "b", 6);
        assert_eq!(r, ["b", "a"]);
        let r = (0..10).fold(Vec::new(), |r, i| remember(&r, &i.to_string(), 6));
        assert_eq!(r.len(), 6);
        assert_eq!(r[0], "9");
    }

    #[test]
    fn unknown_recent_ids_are_skipped() {
        let c = catalog();
        let idx = recent_entries(&c, &["gone".into(), "lavender".into()]);
        assert_eq!(idx.len(), 1);
        assert_eq!(c[idx[0]].id, "lavender");
    }
}
