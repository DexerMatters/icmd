// Editor model tests: cursor movement, insertion, paste, and controlled versus
// uncontrolled value ownership. The model runs inside the input component, so
// its internals are reached through the hidden module.
use icmd::__private::{
    EditAction, EditIntent, EditModel, EditOutcome, EditPolicy, Outcome, ValueOwnership, normalize,
};
use icmd::EmojiMerging;

use unicode_segmentation::UnicodeSegmentation;

fn uncontrolled(value: &str, multiline: bool) -> EditModel {
    uncontrolled_with(value, multiline, EmojiMerging::Merge)
}

fn controlled(value: &str, multiline: bool) -> EditModel {
    controlled_with(value, multiline, EmojiMerging::Merge)
}

fn uncontrolled_with(value: &str, multiline: bool, merging: EmojiMerging) -> EditModel {
    let mut model = EditModel::default();
    model.render(None, Some(value), multiline, merging);
    model
}

fn controlled_with(value: &str, multiline: bool, merging: EmojiMerging) -> EditModel {
    let mut model = EditModel::default();
    model.render(Some(value), None, multiline, merging);
    model
}

fn policy(multiline: bool) -> EditPolicy {
    EditPolicy {
        multiline,
        emoji_merging: EmojiMerging::Merge,
        ..EditPolicy::default()
    }
}

fn separate_policy(multiline: bool) -> EditPolicy {
    EditPolicy {
        multiline,
        emoji_merging: EmojiMerging::Separate,
        ..EditPolicy::default()
    }
}

fn insert(model: &mut EditModel, text: &str, multiline: bool) -> EditOutcome {
    model.reduce(EditAction::Insert(text.to_string()), policy(multiline))
}

#[test]
fn insert_replaces_the_selection() {
    let mut model = uncontrolled("hello", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    model.reduce(
        EditAction::PlaceCaret {
            offset: 4,
            extend: true,
        },
        policy(false),
    );
    let outcome = insert(&mut model, "i", false);
    assert_eq!(model.value(), "hio");
    assert_eq!(outcome.value.as_deref(), Some("hio"));
    assert_eq!(model.caret().cursor, 2);
}

#[test]
fn backspace_and_delete_remove_one_grapheme() {
    let mut model = uncontrolled("a界b", false);
    // Caret after the wide grapheme (offset 4).
    model.reduce(
        EditAction::PlaceCaret {
            offset: 4,
            extend: false,
        },
        policy(false),
    );
    insert(&mut model, "X", false);
    assert_eq!(model.value(), "a界Xb");

    let outcome = model.reduce(EditAction::Backspace { word: false }, policy(false));
    assert_eq!(model.value(), "a界b");
    // Removed text is retained for undo, but Backspace is not a Cut.
    assert_eq!(outcome.removed_text.as_deref(), Some("X"));
    assert_eq!(outcome.intent, EditIntent::DeleteBackward);

    let outcome = model.reduce(EditAction::Delete { word: false }, policy(false));
    assert_eq!(model.value(), "a界");
    assert_eq!(outcome.removed_text.as_deref(), Some("b"));
    assert_eq!(outcome.intent, EditIntent::DeleteForward);
}

#[test]
fn only_cut_reports_cut_intent() {
    let mut model = uncontrolled("hello", false);
    model.reduce(EditAction::SelectAll, policy(false));
    let outcome = model.reduce(EditAction::Cut, policy(false));
    assert_eq!(model.value(), "");
    assert_eq!(outcome.removed_text.as_deref(), Some("hello"));
    assert_eq!(outcome.intent, EditIntent::Cut);

    // A collapsed caret has nothing to cut and stays a no-op.
    let mut model = uncontrolled("abc", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    let outcome = model.reduce(EditAction::Cut, policy(false));
    assert!(!outcome.changed);
    assert_eq!(outcome.intent, EditIntent::None);
    assert_eq!(model.value(), "abc");
}

#[test]
fn paste_reports_paste_intent() {
    let mut model = uncontrolled("ab", false);
    let outcome = model.reduce(EditAction::Paste("c".to_string()), policy(false));
    assert_eq!(model.value(), "abc");
    assert_eq!(outcome.intent, EditIntent::Paste);
}

#[test]
fn home_and_end_move_within_the_logical_line() {
    let mut model = uncontrolled("abc\ndef", true);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 6,
            extend: false,
        },
        policy(true),
    );
    model.reduce(
        EditAction::Home {
            extend: false,
            document: false,
        },
        policy(true),
    );
    assert_eq!(model.caret().cursor, 4);
    model.reduce(
        EditAction::End {
            extend: false,
            document: false,
        },
        policy(true),
    );
    assert_eq!(model.caret().cursor, 7);
    model.reduce(
        EditAction::Home {
            extend: false,
            document: true,
        },
        policy(true),
    );
    assert_eq!(model.caret().cursor, 0);
    model.reduce(
        EditAction::End {
            extend: false,
            document: true,
        },
        policy(true),
    );
    assert_eq!(model.caret().cursor, 7);
}

#[test]
fn select_all_covers_the_whole_value() {
    let mut model = uncontrolled("abc", false);
    model.reduce(EditAction::SelectAll, policy(false));
    assert_eq!(model.caret().range(), (0, 3));
    insert(&mut model, "z", false);
    assert_eq!(model.value(), "z");
}

#[test]
fn left_and_right_collapse_an_existing_selection() {
    let mut model = uncontrolled("abcdef", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    model.reduce(
        EditAction::PlaceCaret {
            offset: 4,
            extend: true,
        },
        policy(false),
    );
    model.reduce(
        EditAction::MoveLeft {
            extend: false,
            word: false,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 1);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    model.reduce(
        EditAction::PlaceCaret {
            offset: 4,
            extend: true,
        },
        policy(false),
    );
    model.reduce(
        EditAction::MoveRight {
            extend: false,
            word: false,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 4);
}

#[test]
fn word_movement_skips_whole_words() {
    let mut model = uncontrolled("hello brave world", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 0,
            extend: false,
        },
        policy(false),
    );
    model.reduce(
        EditAction::MoveRight {
            extend: false,
            word: true,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 5);
    model.reduce(
        EditAction::MoveRight {
            extend: false,
            word: true,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 11);
    model.reduce(
        EditAction::MoveLeft {
            extend: false,
            word: true,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 6);
}

#[test]
fn vertical_targets_are_valid_grapheme_boundaries() {
    let mut model = uncontrolled("abcdef\nxy\nabcdef", true);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 5,
            extend: false,
        },
        policy(true),
    );
    // The component resolves the target from the layout; the model only
    // validates it.
    model.move_vertical(9, false, Some(2));
    assert_eq!(model.caret().cursor, 9);
    assert_eq!(model.preferred_column(), Some(2));
}

#[test]
fn single_line_normalization_maps_newlines_and_tabs_to_spaces() {
    assert_eq!(normalize("a\r\nb\tc\rd", false), "a b c d");
    assert_eq!(normalize("a\u{7}b", false), "ab");
}

#[test]
fn multiline_normalization_keeps_newlines_and_tabs() {
    assert_eq!(normalize("a\r\nb", true), "a\nb");
    assert_eq!(normalize("a\rb", true), "a\nb");
    assert_eq!(normalize("a\tb", true), "a\tb");
    assert_eq!(normalize("a\u{7}b", true), "ab");
}

#[test]
fn paste_is_normalized_before_insertion() {
    let mut model = uncontrolled("", true);
    insert(&mut model, "界\r\n🙂", true);
    assert_eq!(model.value(), "界\n🙂");

    let mut model = uncontrolled("", false);
    insert(&mut model, "a\nb\tc", false);
    assert_eq!(model.value(), "a b c");
}

#[test]
fn max_length_counts_graphemes_and_respects_merging() {
    let mut model = uncontrolled("", false);
    let outcome = model.reduce(
        EditAction::Insert("界界界".into()),
        EditPolicy {
            max_length: Some(2),
            ..policy(false)
        },
    );
    assert_eq!(outcome.value.as_deref(), Some("界界"));

    // Inserting a combining mark after its base must not be rejected when
    // the merged cluster still fits.
    let mut model = uncontrolled("e", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    model.reduce(
        EditAction::Insert("\u{301}".into()),
        EditPolicy {
            max_length: Some(1),
            ..policy(false)
        },
    );
    assert_eq!(model.value(), "e\u{301}");
    assert_eq!(model.value().graphemes(true).count(), 1);
}

#[test]
fn merging_input_extends_the_cluster_under_the_caret() {
    for (base, inserted, merged) in [
        ("👩", "\u{200D}💻", "👩\u{200D}💻"),
        ("👍", "🏽", "👍🏽"),
        ("🇺", "🇸", "🇺🇸"),
        ("❤", "\u{FE0F}", "❤\u{FE0F}"),
        ("1", "\u{FE0F}\u{20E3}", "1\u{FE0F}\u{20E3}"),
    ] {
        let mut model = uncontrolled(base, false);
        assert_eq!(
            model.caret().cursor,
            base.len(),
            "seeded caret is at the end"
        );
        let outcome = insert(&mut model, inserted, false);
        assert_eq!(outcome.value.as_deref(), Some(merged));
        assert_eq!(model.value(), merged);
        assert_eq!(
            model.caret().cursor,
            merged.len(),
            "{merged:?}: the caret sits after the merged cluster"
        );
        assert_eq!(
            model.value().graphemes(true).count(),
            1,
            "{merged:?} is one cluster"
        );
        model.reduce(EditAction::Backspace { word: false }, policy(false));
        assert_eq!(model.value(), "", "one backspace removes the cluster");
    }
}

#[test]
fn pasted_clusters_keep_their_neighbors_and_one_navigation_step() {
    let mut model = uncontrolled("ab", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    let cluster = "👩\u{200D}💻";
    insert(&mut model, cluster, false);
    assert_eq!(model.value(), format!("a{cluster}b"));
    assert_eq!(model.caret().cursor, 1 + cluster.len());
    model.reduce(
        EditAction::MoveLeft {
            extend: false,
            word: false,
        },
        policy(false),
    );
    assert_eq!(model.caret().cursor, 1, "left steps over the whole cluster");
    model.reduce(
        EditAction::MoveRight {
            extend: false,
            word: false,
        },
        policy(false),
    );
    assert_eq!(
        model.caret().cursor,
        1 + cluster.len(),
        "right steps over the whole cluster"
    );
}

#[test]
fn max_length_accepts_emoji_codepoints_that_merge() {
    let mut model = uncontrolled("👍", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: "👍".len(),
            extend: false,
        },
        policy(false),
    );
    let outcome = model.reduce(
        EditAction::Insert("🏽".into()),
        EditPolicy {
            max_length: Some(1),
            ..policy(false)
        },
    );
    assert_eq!(outcome.value.as_deref(), Some("👍🏽"));
    assert_eq!(model.value().graphemes(true).count(), 1);

    // A whole sequence pasted into an empty field still counts as one.
    let mut model = uncontrolled("", false);
    let outcome = model.reduce(
        EditAction::Insert("👨\u{200D}👩\u{200D}👧\u{200D}👦".into()),
        EditPolicy {
            max_length: Some(1),
            ..policy(false)
        },
    );
    assert_eq!(
        outcome.value.as_deref(),
        Some("👨\u{200D}👩\u{200D}👧\u{200D}👦")
    );
}

#[test]
fn separate_mode_places_the_caret_between_parts() {
    let sequence = "👩\u{200D}💻";
    let model = uncontrolled_with(sequence, false, EmojiMerging::Separate);
    // "👩\u{200D}" is 7 bytes, "💻" is 4.
    assert_eq!(model.boundaries(), vec![0, 7, 11]);
    assert_eq!(model.clamp(7), 7, "the part boundary is a real position");
    assert_eq!(model.clamp(3), 0, "inside the first part, nearer its start");
    assert_eq!(
        model.clamp(10),
        11,
        "inside the second part, nearer its end"
    );
    assert_eq!(model.next_boundary(0), 7);
    assert_eq!(model.next_boundary(7), 11);
    assert_eq!(model.previous_boundary(11), 7);
    assert_eq!(model.previous_boundary(7), 0);

    // The merged mode keeps the same value as one cluster.
    let merged = uncontrolled_with(sequence, false, EmojiMerging::Merge);
    assert_eq!(merged.boundaries(), vec![0, 11]);
}

#[test]
fn separate_mode_backspace_removes_one_part() {
    let mut model = uncontrolled_with("👩\u{200D}💻", false, EmojiMerging::Separate);
    assert_eq!(model.caret().cursor, 11, "the caret starts at the end");
    model.reduce(
        EditAction::Backspace { word: false },
        separate_policy(false),
    );
    assert_eq!(model.value(), "👩\u{200D}");
    assert_eq!(model.caret().cursor, 7);
    model.reduce(
        EditAction::Backspace { word: false },
        separate_policy(false),
    );
    assert_eq!(model.value(), "");
}

#[test]
fn separate_mode_inserts_between_parts() {
    let mut model = uncontrolled_with("👩\u{200D}💻", false, EmojiMerging::Separate);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 7,
            extend: false,
        },
        separate_policy(false),
    );
    let outcome = model.reduce(EditAction::Insert("X".into()), separate_policy(false));
    assert_eq!(outcome.value.as_deref(), Some("👩\u{200D}X💻"));
    assert_eq!(model.caret().cursor, 8);
    // Left and right step one part at a time.
    model.reduce(
        EditAction::MoveLeft {
            extend: false,
            word: false,
        },
        separate_policy(false),
    );
    assert_eq!(model.caret().cursor, 7);
    model.reduce(
        EditAction::MoveRight {
            extend: false,
            word: false,
        },
        separate_policy(false),
    );
    assert_eq!(model.caret().cursor, 8);
}

#[test]
fn separate_mode_max_length_counts_parts() {
    let mut model = uncontrolled_with("", false, EmojiMerging::Separate);
    let outcome = model.reduce(
        EditAction::Insert("👩\u{200D}💻".into()),
        EditPolicy {
            max_length: Some(1),
            ..separate_policy(false)
        },
    );
    assert_eq!(outcome.value.as_deref(), Some("👩\u{200D}"));
    assert_eq!(model.value(), "👩\u{200D}");

    let mut model = uncontrolled_with("", false, EmojiMerging::Separate);
    let outcome = model.reduce(
        EditAction::Insert("👩\u{200D}💻".into()),
        EditPolicy {
            max_length: Some(2),
            ..separate_policy(false)
        },
    );
    assert_eq!(outcome.value.as_deref(), Some("👩\u{200D}💻"));
}

#[test]
fn read_only_and_disabled_do_not_mutate() {
    let mut model = uncontrolled("abc", false);
    let read_only = EditPolicy {
        read_only: true,
        ..policy(false)
    };
    model.reduce(EditAction::Insert("x".into()), read_only);
    model.reduce(EditAction::Backspace { word: false }, read_only);
    assert_eq!(model.value(), "abc");

    let disabled = EditPolicy {
        disabled: true,
        ..policy(false)
    };
    let outcome = model.reduce(EditAction::Insert("x".into()), disabled);
    assert!(!outcome.handled);
    assert_eq!(model.value(), "abc");
}

#[test]
fn controlled_acceptance_restores_the_draft_selection() {
    let mut model = controlled("a", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    let outcome = insert(&mut model, "b", false);
    assert_eq!(outcome.value.as_deref(), Some("ab"));
    // The owner accepts by rendering exactly the emitted value.
    let outcome = model.render(Some("ab"), None, false, EmojiMerging::Merge);
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(model.caret().cursor, 2, "accepted draft keeps its caret");
}

#[test]
fn controlled_rejection_clamps_the_selection() {
    let mut model = controlled("abc", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 3,
            extend: false,
        },
        policy(false),
    );
    insert(&mut model, "xy", false);
    assert_eq!(model.value(), "abcxy");
    // A different value is a rejection/external replacement.
    model.render(Some("ab"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "ab");
    assert_eq!(model.caret().cursor, 2);
}

#[test]
fn controlled_rejection_clamps_the_selection_into_the_owner_value() {
    let mut model = controlled("abcd", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 4,
            extend: false,
        },
        policy(false),
    );
    insert(&mut model, "xy", false);
    assert_eq!(model.value(), "abcdxy");
    // The owner republishes a shorter value: the draft is discarded and the
    // selection is clamped into the value the owner supplied.
    model.render(Some("ab"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "ab");
    assert_eq!(model.caret().cursor, 2);
    // The next keystroke edits the owner's value, never the rejected draft.
    let outcome = insert(&mut model, "c", false);
    assert_eq!(outcome.value.as_deref(), Some("abc"));
}

#[test]
fn controlled_rejection_is_deterministic_per_render() {
    // The owner ignores every change, so each render republishes "a". The
    // first keystroke produces "ab"; the next render rejects it back to "a"
    // and clamps the caret to the end of "a". The following keystroke must
    // therefore produce "ac", not resurrect the rejected "ab".
    let mut model = controlled("a", false);
    let mut emitted = Vec::new();
    for ch in ["b", "c"] {
        if let Some(value) = insert(&mut model, ch, false).value {
            emitted.push(value);
        }
        // The owner rejects: it republishes its own authoritative value.
        model.render(Some("a"), None, false, EmojiMerging::Merge);
        assert_eq!(model.value(), "a", "rejection must restore the owner value");
    }
    assert_eq!(
        emitted,
        vec!["ab".to_string(), "ac".to_string()],
        "a rejected draft must not survive into the next keystroke"
    );
}

#[test]
fn acceptance_then_extension_keeps_the_caret() {
    let mut model = controlled("a", false);
    // Click places the caret after 'a'.
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    assert_eq!(insert(&mut model, "x", false).value.as_deref(), Some("ax"));
    assert_eq!(model.caret().cursor, 2);
    // The owner accepts by republishing exactly the draft.
    model.render(Some("ax"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "ax");
    assert_eq!(model.caret().cursor, 2, "acceptance restores the caret");
    // The next keystroke extends the accepted value.
    assert_eq!(insert(&mut model, "y", false).value.as_deref(), Some("axy"));
}

#[test]
fn controlled_external_replacement_discards_the_draft() {
    let mut model = controlled("a", false);
    insert(&mut model, "b", false);
    assert_eq!(model.value(), "ab");
    // The owner publishes an unrelated value. The caret clamps into it at
    // its previous offset (2), and the rejected draft is gone.
    model.render(Some("zzz"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "zzz");
    assert_eq!(model.caret().cursor, 2);
    let outcome = insert(&mut model, "!", false);
    assert_eq!(
        outcome.value.as_deref(),
        Some("zz!z"),
        "editing continues in the owner's value, not the draft"
    );
    assert!(!model.value().contains("ab"), "the draft must be discarded");
}

#[test]
fn rapid_events_before_a_render_accumulate_on_one_draft() {
    let mut model = controlled("", false);
    for ch in ["h", "e", "l", "l", "o"] {
        insert(&mut model, ch, false);
    }
    assert_eq!(model.value(), "hello");
    // A single acceptance render keeps the whole draft.
    model.render(Some("hello"), None, false, EmojiMerging::Merge);
    assert_eq!(model.caret().cursor, 5);
}

#[test]
fn switching_from_uncontrolled_to_controlled_adopts_the_value() {
    let mut model = uncontrolled("local", false);
    model.render(Some("owner"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "owner");
    assert_eq!(model.ownership(), ValueOwnership::Controlled);
}

#[test]
fn switching_to_uncontrolled_discards_an_unaccepted_draft() {
    // The owner published "a" and never accepted the speculative "ab".
    // Switching to uncontrolled must resume from "a", not from a draft the
    // owner rejected by staying silent.
    let mut model = controlled("a", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    insert(&mut model, "b", false);
    assert_eq!(model.value(), "ab");
    // The component stops supplying a value before the owner responded.
    model.render(None, None, false, EmojiMerging::Merge);
    assert_eq!(
        model.value(),
        "a",
        "the unaccepted draft must not become the uncontrolled value"
    );
    assert_eq!(model.ownership(), ValueOwnership::Uncontrolled);
    // Local ownership resumes from that value.
    let outcome = insert(&mut model, "!", false);
    assert_eq!(outcome.value.as_deref(), Some("a!"));
}

#[test]
fn switching_from_controlled_to_uncontrolled_keeps_the_last_value() {
    let mut model = controlled("owner", false);
    model.render(None, None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "owner");
    assert_eq!(model.ownership(), ValueOwnership::Uncontrolled);
    // Local ownership resumes.
    let outcome = insert(&mut model, "!", false);
    assert_eq!(outcome.value.as_deref(), Some("owner!"));
}

#[test]
fn a_draft_is_answered_by_the_next_render_only() {
    // A draft exists only for edits made since the render now being
    // superseded, so the next render is always the owner's answer to it: the
    // caret snapshot is restored there and nowhere else.
    let mut model = controlled("a", false);
    model.reduce(
        EditAction::PlaceCaret {
            offset: 1,
            extend: false,
        },
        policy(false),
    );
    assert_eq!(insert(&mut model, "b", false).value.as_deref(), Some("ab"));

    // The owner republishes the emitted value at the NEXT render: accepted,
    // and the selection that produced the draft is restored.
    let outcome = model.render(Some("ab"), None, false, EmojiMerging::Merge);
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(model.caret().cursor, 2);

    // The owner then replaces the value entirely: no draft is open, so this
    // is a plain replacement rather than an acceptance, and the selection is
    // clamped into the new value rather than restored from a snapshot.
    model.render(Some("xyz"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "xyz");
    assert_eq!(
        model.caret().cursor,
        2,
        "the selection is clamped into the replacement, not reset"
    );
    assert!(model.caret().cursor <= model.value().len());
}

#[test]
fn unrelated_rerender_is_stable() {
    let mut model = controlled("same", false);
    assert_eq!(
        model.render(Some("same"), None, false, EmojiMerging::Merge),
        Outcome::Unchanged
    );
}

#[test]
fn clamp_snaps_to_the_nearest_grapheme_boundary() {
    // "e" is one byte; the combining mark spans bytes 1..3; "x" is byte 3.
    let model = uncontrolled("e\u{301}x", false);
    assert_eq!(model.clamp(0), 0);
    assert_eq!(model.clamp(1), 0, "inside the cluster, nearer the start");
    assert_eq!(model.clamp(2), 3, "inside the cluster, nearer the end");
    assert_eq!(model.clamp(3), 3, "the cluster's trailing boundary");
    assert_eq!(model.clamp(4), 4);
    assert_eq!(model.clamp(usize::MAX), 4);
}

#[test]
fn a_controlled_owner_catching_up_does_not_reset_the_caret() {
    // A controlled owner republishes asynchronously, so a render can carry an
    // earlier link of the emitted chain while the owner is still behind.
    // Treating that as a rejection reverted the value and clamped the caret to
    // the shorter string, which is what made a fast typist - or a paste - land
    // behind the caret.
    let mut model = controlled("a", false);
    assert_eq!(model.caret().cursor, 1);

    // Several edits land before the owner's first answer.
    insert(&mut model, "x", false);
    insert(&mut model, "y", false);
    insert(&mut model, "z", false);
    assert_eq!(model.value(), "axyz");
    assert_eq!(model.caret().cursor, 4);

    // The owner answers one render behind, with the first emission.
    model.render(Some("ax"), None, false, EmojiMerging::Merge);
    assert_eq!(
        model.value(),
        "axyz",
        "a catch-up echo must not revert the optimistic value"
    );
    assert_eq!(
        model.caret().cursor,
        4,
        "a catch-up echo must not clamp the caret"
    );

    // The owner republishes the latest emission: accepted, caret kept.
    model.render(Some("axyz"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "axyz");
    assert_eq!(model.caret().cursor, 4);

    // The next keystroke lands at the end rather than behind the caret.
    insert(&mut model, "X", false);
    assert_eq!(model.value(), "axyzX");
    assert_eq!(model.caret().cursor, 5);
}

#[test]
fn a_controlled_value_the_model_never_emitted_is_still_applied() {
    // A rejection or an external replacement is a value the model never
    // produced, so it must still win over the optimistic draft.
    let mut model = controlled("a", false);
    insert(&mut model, "x", false);
    insert(&mut model, "y", false);
    assert_eq!(model.value(), "axy");

    model.render(Some("a!"), None, false, EmojiMerging::Merge);
    assert_eq!(model.value(), "a!");
    assert_eq!(model.caret().cursor, 2);
}
