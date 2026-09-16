//! Pure state machines behind the guide's interactive demonstrations.
//!
//! Each demonstration keeps its rendering in the chapter and its decisions
//! here, so behavior such as a bounded log, a keyed reorder, a publish timer,
//! the layout breakpoints, and a release checklist can be tested without a
//! terminal.

/// Appends `entry`, dropping the oldest items once `cap` is exceeded.
///
/// Bounded logs are the difference between an interactive ledger and unbounded
/// memory growth, so every accumulating demonstration uses this.
pub(crate) fn push_bounded(log: &mut Vec<String>, entry: String, cap: usize) {
    log.push(entry);
    let cap = cap.max(1);
    while log.len() > cap {
        log.remove(0);
    }
}

/// Returns `items` in reverse display order.
///
/// The keyed task list reverses its order to show that row identity follows the
/// key, not the position.
pub(crate) fn reversed<T: Clone>(items: &[T]) -> Vec<T> {
    let mut next = items.to_vec();
    next.reverse();
    next
}

/// Driver action a publish operation understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishAction {
    /// Begin or resume.
    Start,
    /// Hold at the current progress.
    Pause,
    /// Return to the beginning.
    Reset,
}

/// Phase of the simulated publish operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PublishPhase {
    /// Nothing has started.
    #[default]
    Idle,
    /// The operation is advancing.
    Running,
    /// The operation is held.
    Paused,
    /// The operation completed.
    Done,
}

impl PublishPhase {
    /// Applies one driver action; a completed operation stays completed.
    pub(crate) fn apply(self, action: PublishAction) -> Self {
        match (self, action) {
            (Self::Done, _) => Self::Done,
            (Self::Idle, PublishAction::Start) => Self::Running,
            (Self::Idle, PublishAction::Pause | PublishAction::Reset) => Self::Idle,
            (Self::Running, PublishAction::Pause) => Self::Paused,
            (Self::Running, PublishAction::Start) => Self::Running,
            (Self::Running, PublishAction::Reset) => Self::Idle,
            (Self::Paused, PublishAction::Start) => Self::Running,
            (Self::Paused, PublishAction::Pause) => Self::Paused,
            (Self::Paused, PublishAction::Reset) => Self::Idle,
        }
    }

    /// Whether the worker timer should be alive.
    pub(crate) const fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    /// Short status word; the badge always carries this text as well as color.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "publishing",
            Self::Paused => "paused",
            Self::Done => "published",
        }
    }
}

/// The state one timer tick produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublishTick {
    /// Phase after the tick.
    pub(crate) phase: PublishPhase,
    /// Clamped progress after the tick, `0..=100`.
    pub(crate) progress: u64,
    /// Spinner frame index after the tick.
    pub(crate) frame: usize,
}

/// Advances the simulated publish by one tick.
///
/// Progress is clamped to `100` and the operation completes exactly at `total`
/// ticks, so the demonstration can never run away.
pub(crate) fn publish_tick(tick: usize, progress: u64, step: u64, total: usize) -> PublishTick {
    let progress = progress.saturating_add(step).min(100);
    if tick >= total.max(1) {
        PublishTick {
            phase: PublishPhase::Done,
            progress: 100,
            frame: tick,
        }
    } else {
        PublishTick {
            phase: PublishPhase::Running,
            progress,
            frame: tick,
        }
    }
}

/// Release-checklist state: session-local, resettable, and counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Checklist {
    done: Vec<bool>,
}

impl Checklist {
    /// Creates a checklist of `items` unchecked entries.
    pub(crate) fn new(items: usize) -> Self {
        Self {
            done: vec![false; items],
        }
    }

    /// Flips one entry; out-of-range indices are ignored.
    pub(crate) fn toggle(&mut self, index: usize) {
        if let Some(slot) = self.done.get_mut(index) {
            *slot = !*slot;
        }
    }

    /// Clears every entry.
    pub(crate) fn reset(&mut self) {
        self.done.fill(false);
    }

    /// Whether entry `index` is checked.
    pub(crate) fn is_done(&self, index: usize) -> bool {
        self.done.get(index).copied().unwrap_or(false)
    }

    /// Number of checked entries.
    pub(crate) fn completed(&self) -> usize {
        self.done.iter().filter(|done| **done).count()
    }

    /// Number of entries.
    pub(crate) fn total(&self) -> usize {
        self.done.len()
    }

    /// Completion percentage in `0..=100`.
    pub(crate) fn percent(&self) -> u64 {
        if self.done.is_empty() {
            return 0;
        }
        (self.completed() as u64 * 100) / self.done.len() as u64
    }
}

/// Lifecycle phases in execution order, shown as a timeline.
pub(crate) const LIFECYCLE_PHASES: [&str; 5] = ["Boot", "Mount", "Ready", "Unmount", "Exit"];

/// Where a listener runs in the dispatch order.
pub(crate) const EVENT_PHASES: [&str; 3] = ["capture", "target", "bubble"];

/// Root width at or above which the expanded editorial index fits.
pub(crate) const WIDE_BREAKPOINT: u16 = 110;
/// Root width below which the index disappears entirely.
pub(crate) const NARROW_BREAKPOINT: u16 = 72;
/// Expanded editorial index width in cells.
pub(crate) const INDEX_WIDTH: u16 = 30;
/// Compact numbered rail width in cells.
pub(crate) const RAIL_WIDTH: u16 = 7;

/// The layout treatment a measured width selects.
///
/// This is the one source of truth for the documentation shell's breakpoints
/// and for the responsive chapter that teaches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WidthMode {
    /// Room for an expanded index beside a wide document column.
    Wide,
    /// Room for a compact rail only.
    Mixed,
    /// No index; navigation lives in the header and search.
    Narrow,
}

impl WidthMode {
    /// Chooses the mode for a measured width, honouring a collapse preference
    /// only where the viewport can hold the expanded index.
    pub(crate) const fn for_width(width: u16, collapsed: bool) -> Self {
        if width >= WIDE_BREAKPOINT && !collapsed {
            Self::Wide
        } else if width >= NARROW_BREAKPOINT {
            Self::Mixed
        } else {
            Self::Narrow
        }
    }

    /// Cells the effective mode reserves from the content column.
    pub(crate) const fn reserved_width(self) -> u16 {
        match self {
            Self::Wide => INDEX_WIDTH,
            Self::Mixed => RAIL_WIDTH,
            Self::Narrow => 0,
        }
    }

    /// Human-readable mode name with a symbolic cue.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Wide => "wide · expanded index",
            Self::Mixed => "medium · compact rail",
            Self::Narrow => "narrow · content only",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_log_never_exceeds_its_cap() {
        let mut log = Vec::new();
        for index in 0..25 {
            push_bounded(&mut log, format!("entry {index}"), 8);
        }
        assert_eq!(log.len(), 8);
        assert_eq!(log.first().map(String::as_str), Some("entry 17"));
        assert_eq!(log.last().map(String::as_str), Some("entry 24"));
    }

    #[test]
    fn bounded_log_keeps_everything_under_the_cap() {
        let mut log = Vec::new();
        push_bounded(&mut log, String::from("only"), 4);
        assert_eq!(log, vec![String::from("only")]);
    }

    #[test]
    fn reversing_preserves_identity_of_each_item() {
        let items = vec!["a", "b", "c"];
        let next = reversed(&items);
        assert_eq!(next, vec!["c", "b", "a"]);
        assert_eq!(items, vec!["a", "b", "c"]);
        let mut sorted = next.clone();
        sorted.sort_unstable();
        let mut original = items.clone();
        original.sort_unstable();
        assert_eq!(sorted, original);
    }

    #[test]
    fn publish_phase_transitions_are_total_and_completion_is_sticky() {
        for action in [
            PublishAction::Start,
            PublishAction::Pause,
            PublishAction::Reset,
        ] {
            assert_eq!(PublishPhase::Done.apply(action), PublishPhase::Done);
        }
        assert_eq!(
            PublishPhase::Idle.apply(PublishAction::Start),
            PublishPhase::Running
        );
        assert_eq!(
            PublishPhase::Running.apply(PublishAction::Pause),
            PublishPhase::Paused
        );
        assert_eq!(
            PublishPhase::Paused.apply(PublishAction::Start),
            PublishPhase::Running
        );
        assert_eq!(
            PublishPhase::Running.apply(PublishAction::Reset),
            PublishPhase::Idle
        );
        assert_eq!(
            PublishPhase::Paused.apply(PublishAction::Reset),
            PublishPhase::Idle
        );
        assert_eq!(
            PublishPhase::Idle.apply(PublishAction::Pause),
            PublishPhase::Idle
        );
    }

    #[test]
    fn publish_tick_clamps_progress_and_completes_exactly_once() {
        let mut progress = 0;
        let mut phase = PublishPhase::Idle;
        for tick in 1..=20 {
            let next = publish_tick(tick, progress, 5, 20);
            progress = next.progress;
            phase = next.phase;
            assert!(progress <= 100);
        }
        assert_eq!(progress, 100);
        assert_eq!(phase, PublishPhase::Done);
        let beyond = publish_tick(21, progress, 5, 20);
        assert_eq!(beyond.phase, PublishPhase::Done);
        assert_eq!(beyond.progress, 100);
        assert_eq!(PublishPhase::Done.label(), "published");
        assert!(PublishPhase::Running.is_running());
        assert!(!PublishPhase::Paused.is_running());
    }

    #[test]
    fn checklist_reducer_is_bounded_and_resettable() {
        let mut checklist = Checklist::new(4);
        assert_eq!(checklist.total(), 4);
        assert_eq!(checklist.completed(), 0);
        assert_eq!(checklist.percent(), 0);
        checklist.toggle(1);
        checklist.toggle(3);
        assert!(checklist.is_done(1));
        assert!(!checklist.is_done(0));
        assert_eq!(checklist.completed(), 2);
        assert_eq!(checklist.percent(), 50);
        checklist.toggle(99);
        assert_eq!(checklist.completed(), 2);
        checklist.reset();
        assert_eq!(checklist.completed(), 0);
        assert_eq!(Checklist::default().percent(), 0);
    }

    #[test]
    fn width_mode_matches_the_documented_breakpoints() {
        assert_eq!(WidthMode::for_width(140, false), WidthMode::Wide);
        assert_eq!(WidthMode::for_width(140, true), WidthMode::Mixed);
        assert_eq!(WidthMode::for_width(110, false), WidthMode::Wide);
        assert_eq!(WidthMode::for_width(109, false), WidthMode::Mixed);
        assert_eq!(WidthMode::for_width(72, false), WidthMode::Mixed);
        assert_eq!(WidthMode::for_width(71, false), WidthMode::Narrow);
        assert_eq!(WidthMode::Wide.reserved_width(), INDEX_WIDTH);
        assert_eq!(WidthMode::Mixed.reserved_width(), RAIL_WIDTH);
        assert_eq!(WidthMode::Narrow.reserved_width(), 0);
        assert!(!WidthMode::Narrow.label().is_empty());
    }
}
