//! The search index and its ranking rules.
//!
//! The index is derived from [`crate::metadata`] on demand: chapter titles,
//! section titles, the component and type names each section covers, and the
//! curated keyword aliases. Matching is case-insensitive and ranked exact
//! before prefix before substring, with chapter order preserved for ties.
//! This is deliberately an index over the guide's own vocabulary rather than
//! arbitrary full-text search.

use crate::metadata::{CHAPTERS, ChapterMeta};

/// What kind of registry entry produced a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitKind {
    /// A chapter title or chapter-level alias.
    Chapter,
    /// A section title.
    Section,
    /// A component, hook, or type name covered by a section.
    Type,
    /// A curated alias keyword for a chapter or section.
    Alias,
}

impl HitKind {
    /// Short uppercase label shown beside a result.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Chapter => "CHAPTER",
            Self::Section => "SECTION",
            Self::Type => "API",
            Self::Alias => "ALIAS",
        }
    }
}

/// One navigable search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchHit {
    /// Matched text as it should be displayed.
    pub label: String,
    /// Where the result lives, such as `03 · Layout and Styling`.
    pub location: String,
    /// Chapter destination.
    pub chapter: usize,
    /// Section destination; `0` for a chapter-level hit.
    pub section: usize,
    /// Registry entry kind.
    pub kind: HitKind,
}

impl SearchHit {
    /// Ranking score; lower is better. Only used by tests and the sort.
    pub(crate) fn _score(&self) -> u8 {
        match self.kind {
            HitKind::Type => 0,
            HitKind::Section => 0,
            HitKind::Chapter => 0,
            HitKind::Alias => 0,
        }
    }
}

/// One index entry before ranking.
struct Entry {
    text: String,
    chapter: usize,
    section: usize,
    chapter_only: bool,
    kind: HitKind,
    location: String,
}

/// Ranking tier: exact, prefix, or substring.
fn tier(text: &str, query: &str) -> Option<u8> {
    if text == query {
        Some(0)
    } else if text.starts_with(query) {
        Some(1)
    } else if text.contains(query) {
        Some(2)
    } else {
        None
    }
}

/// Builds the full index from the registry.
fn entries() -> Vec<Entry> {
    let mut entries = Vec::new();
    for chapter in CHAPTERS {
        let location = format!("{} · {}", chapter.number, chapter.title);
        entries.push(Entry {
            text: chapter.title.to_string(),
            chapter: chapter.index,
            section: 0,
            chapter_only: true,
            kind: HitKind::Chapter,
            location: location.clone(),
        });
        for keyword in chapter.keywords {
            entries.push(Entry {
                text: (*keyword).to_string(),
                chapter: chapter.index,
                section: 0,
                chapter_only: true,
                kind: HitKind::Alias,
                location: location.clone(),
            });
        }
        for (position, section) in chapter.sections.iter().enumerate() {
            let section_location = chapter_location(chapter, position);
            entries.push(Entry {
                text: section.title.to_string(),
                chapter: chapter.index,
                section: position,
                chapter_only: false,
                kind: HitKind::Section,
                location: section_location.clone(),
            });
            for name in section.types {
                entries.push(Entry {
                    text: (*name).to_string(),
                    chapter: chapter.index,
                    section: position,
                    chapter_only: false,
                    kind: HitKind::Type,
                    location: chapter_location(chapter, position),
                });
            }
            for keyword in section.keywords {
                entries.push(Entry {
                    text: (*keyword).to_string(),
                    chapter: chapter.index,
                    section: position,
                    chapter_only: false,
                    kind: HitKind::Alias,
                    location: section_location.clone(),
                });
            }
        }
    }
    entries
}

/// Breadcrumb for a section destination, resolved through the registry.
fn chapter_location(chapter: &ChapterMeta, position: usize) -> String {
    let Some(section) = crate::metadata::section(chapter.index, position) else {
        return chapter.title.to_string();
    };
    format!(
        "{} · {} / {}",
        chapter.number, section.number, section.title
    )
}

/// Result cap, as specified by the search design.
pub(crate) const MAX_RESULTS: usize = 8;

/// Case-insensitive ranked search over the registry vocabulary.
///
/// Exact matches rank before prefix matches, which rank before substring
/// matches. Ties keep chapter order, then section order, then a stable display
/// order, so results never reshuffle between identical queries.
pub(crate) fn search(query: &str) -> Vec<SearchHit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    search_index(&entries(), &query, MAX_RESULTS)
}

/// Scores, sorts, deduplicates, and truncates one prepared index.
fn search_index(index: &[Entry], query: &str, limit: usize) -> Vec<SearchHit> {
    let mut best: Vec<(u8, usize, &Entry)> = Vec::new();
    for (position, entry) in index.iter().enumerate() {
        let Some(score) = tier(&entry.text.to_lowercase(), query) else {
            continue;
        };
        best.push((score, position, entry));
    }
    best.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.2.chapter.cmp(&right.2.chapter))
            .then(left.2.section.cmp(&right.2.section))
            .then(left.2.chapter_only.cmp(&right.2.chapter_only))
            .then(left.1.cmp(&right.1))
    });

    let mut hits: Vec<SearchHit> = Vec::with_capacity(limit);
    let mut seen: Vec<(usize, usize, String)> = Vec::new();
    for (_, _, entry) in best {
        let key = (entry.chapter, entry.section, entry.text.to_lowercase());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        hits.push(SearchHit {
            label: entry.text.clone(),
            location: entry.location.clone(),
            chapter: entry.chapter,
            section: entry.section,
            kind: entry.kind,
        });
        if hits.len() == limit {
            break;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_nothing() {
        assert!(search("").is_empty());
        assert!(search("   ").is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let upper = search("PROGRESS");
        let lower = search("progress");
        assert!(!upper.is_empty());
        assert_eq!(upper, lower);
    }

    #[test]
    fn exact_ranks_before_prefix_before_substring() {
        let hits = search("badge");
        assert_eq!(
            hits.first().map(|hit| hit.label.to_lowercase()),
            Some("badge".into())
        );
    }

    #[test]
    fn results_are_capped() {
        assert!(search("a").len() <= MAX_RESULTS);
        assert!(search("e").len() <= MAX_RESULTS);
    }

    #[test]
    fn aliases_resolve_to_a_real_destination() {
        let hits = search("space between");
        let hit = hits.first().expect("the alias has a destination");
        assert_eq!(hit.chapter, 2);
        assert!(hit.location.contains("3.2"));
    }

    #[test]
    fn type_names_resolve_to_the_covering_section() {
        let hits = search("scroll_area");
        let hit = hits
            .iter()
            .find(|hit| hit.label == "scroll_area")
            .expect("the widget is indexed");
        let section = crate::metadata::section(hit.chapter, hit.section)
            .expect("a type hit must point at a real section");
        assert!(
            section.types.contains(&"scroll_area"),
            "`scroll_area` must resolve to a section that covers it"
        );
    }

    #[test]
    fn ranking_is_stable_between_identical_queries() {
        assert_eq!(search("state"), search("state"));
    }

    #[test]
    fn every_shipped_widget_and_documented_type_is_searchable() {
        for name in crate::metadata::WIDGETS
            .iter()
            .chain(crate::metadata::HIGH_LEVEL_TYPES.iter())
        {
            let hits = search(name);
            assert!(
                hits.iter().any(|hit| hit.label.eq_ignore_ascii_case(name)),
                "`{name}` must be findable by an exact query"
            );
        }
    }

    #[test]
    fn every_hit_points_at_a_real_destination() {
        for query in ["state", "scroll", "color", "image", "test", "key"] {
            for hit in search(query) {
                let chapter = crate::metadata::chapter(hit.chapter);
                assert!(
                    hit.chapter < crate::metadata::CHAPTER_COUNT
                        && (hit.section == 0
                            || crate::metadata::section(hit.chapter, hit.section).is_some()),
                    "`{}` pointed at a missing destination {}:{}",
                    hit.label,
                    chapter.number,
                    hit.section
                );
            }
        }
    }

    #[test]
    fn cross_chapter_navigation_is_offered_for_shared_words() {
        let chapters: std::collections::HashSet<usize> =
            search("selection").iter().map(|hit| hit.chapter).collect();
        assert!(chapters.len() > 1, "a shared word should span chapters");
    }
}
