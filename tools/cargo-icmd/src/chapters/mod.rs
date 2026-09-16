//! Chapter routing, the shared scroll viewport, navigation, and scroll-spy.
//!
//! Each chapter is an ordinary component returning an arbitrary [`Node`] tree.
//! The shared machinery is the document viewport, the tracked section wrapper
//! that reports the active section, a [`Navigator`] that both the shell and
//! chapter route cards drive, and the pending-section target that lets a search
//! result land in a chapter whose refs have not mounted yet.

mod ch01_start;
mod ch02_state;
mod ch03_layout;
mod ch04_text;
mod ch05_controls;
mod ch06_feedback;
mod ch07_media;
mod ch08_themes;
mod ch09_production;
mod ch10_api;

use std::sync::{Arc, Mutex};

use icmd::theme::Theme;
use icmd::{
    Align, BorderKind, ButtonVariant, Component, Dimension, Edges, ElementRef, ElementSnapshot,
    Justify, Layout, Node, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, StateSetter,
    Text, TextOverflow, TextWrap, button, muted, row, scroll_area, selection_area, ui, view,
};

use crate::docs::section_intro;
use crate::metadata::{self, ChapterMeta, SectionMeta};

/// The shell state a navigator drives.
///
/// Grouping the setters and shared cells keeps the navigator's constructor to a
/// handful of arguments and makes the binding set reusable by tests.
#[derive(Clone)]
pub(crate) struct NavBindings {
    /// Chapter setter.
    pub(crate) set_chapter: StateSetter<usize>,
    /// Active-section setter.
    pub(crate) set_active: StateSetter<usize>,
    /// Controlled scroll-offset setter.
    pub(crate) set_scroll: StateSetter<ScrollOffset>,
    /// Shared active-section value.
    pub(crate) active_ref: Arc<Mutex<usize>>,
    /// Shared pending-section destination.
    pub(crate) pending: Arc<Mutex<Option<usize>>>,
}

impl NavBindings {
    /// Builds one binding set from the shell's state handles.
    pub(crate) fn new(
        set_chapter: StateSetter<usize>,
        set_active: StateSetter<usize>,
        set_scroll: StateSetter<ScrollOffset>,
        active_ref: Arc<Mutex<usize>>,
        pending: Arc<Mutex<Option<usize>>>,
    ) -> Self {
        Self {
            set_chapter,
            set_active,
            set_scroll,
            active_ref,
            pending,
        }
    }
}

/// Shared navigation target used by the shell, the index, and route cards.
///
/// It owns clones of the shell's state setters, so any component can move the
/// guide without receiving the whole shell state.
#[derive(Clone)]
pub(crate) struct Navigator {
    chapter_index: usize,
    section_refs: Vec<ElementRef>,
    viewport_ref: ElementRef,
    bindings: NavBindings,
}

impl Navigator {
    /// Builds a navigator over the current chapter's committed refs.
    pub(crate) fn new(
        chapter_index: usize,
        section_refs: Vec<ElementRef>,
        viewport_ref: ElementRef,
        bindings: NavBindings,
    ) -> Self {
        Self {
            chapter_index,
            section_refs,
            viewport_ref,
            bindings,
        }
    }

    /// The chapter currently mounted.
    pub(crate) fn chapter_index(&self) -> usize {
        self.chapter_index
    }

    /// Number of tracked sections in the mounted chapter.
    pub(crate) fn section_count(&self) -> usize {
        self.section_refs.len()
    }

    /// The document scroll viewport.
    pub(crate) fn viewport_ref(&self) -> &ElementRef {
        &self.viewport_ref
    }

    /// One ref per section in the mounted chapter.
    pub(crate) fn section_refs(&self) -> &[ElementRef] {
        &self.section_refs
    }

    /// Setter for the controlled document scroll offset.
    pub(crate) fn set_scroll(&self) -> &StateSetter<ScrollOffset> {
        &self.bindings.set_scroll
    }

    /// Setter for the active-section indicator.
    pub(crate) fn set_active(&self) -> &StateSetter<usize> {
        &self.bindings.set_active
    }

    /// Shared active-section value, read without a render.
    pub(crate) fn active_ref(&self) -> &Arc<Mutex<usize>> {
        &self.bindings.active_ref
    }

    /// Shared pending-section destination, if a navigation is waiting for refs.
    pub(crate) fn pending(&self) -> &Arc<Mutex<Option<usize>>> {
        &self.bindings.pending
    }

    /// Switches chapter and resets the document to its top.
    pub(crate) fn goto_chapter(&self, chapter: usize) {
        if chapter == self.chapter_index || chapter >= metadata::CHAPTER_COUNT {
            return;
        }
        if let Ok(mut pending) = self.bindings.pending.lock() {
            *pending = None;
        }
        if let Ok(mut active) = self.bindings.active_ref.lock() {
            *active = 0;
        }
        self.bindings.set_chapter.set(chapter);
        self.bindings.set_active.set(0);
        self.bindings.set_scroll.set(ScrollOffset::default());
    }

    /// Scrolls to a section in the mounted chapter.
    pub(crate) fn goto_section(&self, section: usize) {
        let Some(section_ref) = self.section_refs.get(section) else {
            return;
        };
        match scroll_target(&self.viewport_ref, section_ref) {
            Some(target) => {
                if let Ok(mut active) = self.bindings.active_ref.lock() {
                    *active = section;
                }
                self.bindings.set_active.set(section);
                self.bindings.set_scroll.set(target);
            }
            None => {
                if let Ok(mut pending) = self.bindings.pending.lock() {
                    *pending = Some(section);
                }
            }
        }
    }

    /// Navigates to a destination, parking it when the chapter is not mounted.
    pub(crate) fn goto(&self, chapter: usize, section: usize) {
        if chapter == self.chapter_index {
            self.goto_section(section);
            return;
        }
        if chapter >= metadata::CHAPTER_COUNT {
            return;
        }
        if let Ok(mut pending) = self.bindings.pending.lock() {
            *pending = Some(section);
        }
        if let Ok(mut active) = self.bindings.active_ref.lock() {
            *active = 0;
        }
        self.bindings.set_chapter.set(chapter);
        self.bindings.set_active.set(0);
        self.bindings.set_scroll.set(ScrollOffset::default());
    }
}

/// Runtime bindings the documentation shell supplies to a chapter.
#[derive(Clone)]
pub(crate) struct ChapterProps {
    navigator: Navigator,
    scroll_offset: ScrollOffset,
    wide: bool,
}

impl ChapterProps {
    /// Builds the binding set for one mounted chapter.
    pub(crate) fn new(navigator: Navigator, scroll_offset: ScrollOffset, wide: bool) -> Self {
        Self {
            navigator,
            scroll_offset,
            wide,
        }
    }

    /// The shared navigator, used by route cards and next-passage links.
    pub(crate) fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    /// The controlled document scroll offset.
    pub(crate) fn scroll_offset(&self) -> ScrollOffset {
        self.scroll_offset
    }

    /// Whether the document column has room for a two-column composition.
    pub(crate) fn wide(&self) -> bool {
        self.wide
    }

    /// The document scroll viewport.
    pub(crate) fn viewport_ref(&self) -> &ElementRef {
        self.navigator.viewport_ref()
    }

    /// One ref per section in the mounted chapter.
    pub(crate) fn section_refs(&self) -> &[ElementRef] {
        self.navigator.section_refs()
    }

    /// Setter for the controlled document scroll offset.
    pub(crate) fn set_scroll(&self) -> &StateSetter<ScrollOffset> {
        self.navigator.set_scroll()
    }

    /// Setter for the active-section indicator.
    pub(crate) fn set_active(&self) -> &StateSetter<usize> {
        self.navigator.set_active()
    }

    /// Shared active-section value, read without a render.
    pub(crate) fn active_ref(&self) -> &Arc<Mutex<usize>> {
        self.navigator.active_ref()
    }

    /// Shared pending-section destination.
    pub(crate) fn pending(&self) -> &Arc<Mutex<Option<usize>>> {
        self.navigator.pending()
    }

    /// Number of tracked sections in the mounted chapter.
    pub(crate) fn section_count(&self) -> usize {
        self.navigator.section_count()
    }
}

/// Mounts the chapter component selected by the routing layer.
pub(crate) fn selected_chapter(index: usize, props: ChapterProps) -> Node {
    match index {
        0 => ch01_start::start_here.apply(props),
        1 => ch02_state::components_and_state.apply(props),
        2 => ch03_layout::layout_and_styling.apply(props),
        3 => ch04_text::text_and_documents.apply(props),
        4 => ch05_controls::controls_and_events.apply(props),
        5 => ch06_feedback::feedback_and_canvas.apply(props),
        6 => ch07_media::scroll_selection_media.apply(props),
        7 => ch08_themes::themes.apply(props),
        8 => ch09_production::production.apply(props),
        9 => ch10_api::api_map.apply(props),
        _ => ch01_start::start_here.apply(props),
    }
}

/// Places any chapter tree inside the shared scroll and selection region.
pub(super) fn document(props: &ChapterProps, content: Node) -> Node {
    let viewport_ref = props.viewport_ref().clone();
    let scroll_offset = props.scroll_offset();
    let set_scroll = props.set_scroll().clone();
    let props_for_change = props.clone();
    let viewport_for_scroll = props.viewport_ref().clone();
    ui! {
        <scroll_area
            element_ref={viewport_ref}
            axes={ScrollAxes::Vertical}
            offset={scroll_offset}
            scrollbar_visibility={ScrollbarVisibility::Auto}
            wheel_step={2_u16}
            on_scroll={move |event: ScrollEvent| {
                // Scroll events bubble, which a nested scroll host relies on to
                // observe the region it owns. The document has to do the
                // reverse check: a demonstration that scrolls its own build log
                // also raises an event here, and adopting that offset as the
                // document's would yank the page back to the demonstration.
                // The document's own event is the one whose range matches the
                // range this viewport committed.
                let owned = viewport_for_scroll
                    .current()
                    .and_then(|snapshot| snapshot.scroll())
                    .is_some_and(|scroll| scroll.max_offset == event.max_offset);
                if owned {
                    set_scroll.set(event.offset);
                }
            }}
            on_element_change={move |_snapshot: Option<ElementSnapshot>| {
                if !resolve_pending(&props_for_change) {
                    refresh_active(&props_for_change);
                }
            }}
            style={|style| {
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
            }}
        >
            <selection_area style={|style| style.width /= Dimension::Max}>
                {content}
            </selection_area>
        </scroll_area>
    }
}

/// One teaching section: chapter masthead rhythm, content, and a spy anchor.
pub(super) fn section(
    theme: &Theme,
    props: &ChapterProps,
    index: usize,
    meta: &SectionMeta,
    content: Node,
) -> Node {
    let background = theme.colors.background;
    let inner = ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges { top: 2, right: 4, bottom: 2, left: 4 };
            style.background /= background;
        }}>
            {section_intro(theme, meta)}
            {content}
        </view>
    };
    tracked(props, index, meta.id, inner)
}

/// Attaches one scroll-spy anchor keyed by the section's stable id.
pub(super) fn tracked(props: &ChapterProps, index: usize, key: &str, content: Node) -> Node {
    let section_ref = props.section_refs().get(index).cloned().unwrap_or_default();
    let props_for_change = props.clone();
    let key = key.to_string();
    ui! {
        <view
            key={key}
            element_ref={section_ref}
            on_element_change={move |_snapshot: Option<ElementSnapshot>| {
                if !resolve_pending(&props_for_change) {
                    refresh_active(&props_for_change);
                }
            }}
            style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
            }}
        >
            {content}
        </view>
    }
}

/// Chapter masthead: number, kicker, title, and purpose.
pub(super) fn masthead(theme: &Theme, meta: &ChapterMeta, section_count: usize) -> Node {
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    let primary = theme.colors.primary;
    let secondary = theme.colors.secondary;
    let number = meta.number;
    let kicker = meta.kicker;
    let title = meta.title;
    let summary = meta.summary;
    let label = format!("{section_count} sections");
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges { top: 1, right: 4, bottom: 1, left: 4 };
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Heavy;
            style.border.edges /= Edges { top: false, right: false, bottom: true, left: false };
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new(format!("{number}  {kicker}")).foreground(primary).bold()}
                <muted>{Text::new(label)}</muted>
            </row>
            {Text::new(title).foreground(card_foreground).bold().wrap(TextWrap::Soft)}
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.border.kind /= BorderKind::Heavy;
                style.border.edges /= Edges { top: false, right: false, bottom: false, left: true };
                style.border.foreground /= primary;
                style.padding /= Edges { top: 0, right: 0, bottom: 0, left: 1 };
            }}>
                {Text::new(summary).foreground(secondary).bold().wrap(TextWrap::Soft)}
            </view>
        </view>
    }
}

/// A route card that moves the guide to another chapter.
pub(super) fn route_card(
    theme: &Theme,
    props: &ChapterProps,
    chapter: usize,
    glyph: &str,
    title: &str,
    detail: &str,
) -> Node {
    let navigator = props.navigator().clone();
    let glyph = glyph.to_string();
    let title = title.to_string();
    let detail = detail.to_string();
    let primary = theme.colors.primary;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    ui! {
        <button variant={ButtonVariant::Secondary} on_press={move |_| navigator.goto_chapter(chapter)}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                style.background /= card;
                style.text.foreground /= card_foreground;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
                style.border.background /= card;
            }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                {Text::new(glyph).foreground(primary).bold()}
                {Text::new(title).foreground(primary).bold().overflow(TextOverflow::Ellipsis)}
            </row>
            {Text::new(detail).foreground(card_foreground).wrap(TextWrap::Soft)}
        </button>
    }
}

/// Recomputes the active section from committed geometry and reports a change.
fn refresh_active(props: &ChapterProps) {
    let Some(next) = measured_active_section(props.viewport_ref(), props.section_refs()) else {
        return;
    };
    let Ok(mut current) = props.active_ref().lock() else {
        return;
    };
    if *current != next {
        *current = next;
        props.set_active().set(next);
    }
}

/// Resolves a pending section destination once its refs have committed.
///
/// A search result can name a section in a chapter that is not mounted yet, so
/// the destination is parked and applied by the first anchor that publishes a
/// snapshot after the new chapter commits. Returns whether a destination was
/// applied, so the caller can skip the scroll-spy pass that would otherwise
/// recompute from pre-scroll geometry.
fn resolve_pending(props: &ChapterProps) -> bool {
    let target = match props.pending().lock() {
        Ok(guard) => *guard,
        Err(_) => return false,
    };
    let Some(target) = target else {
        return false;
    };
    if target >= props.section_count() {
        if let Ok(mut pending) = props.pending().lock() {
            *pending = None;
        }
        return false;
    }
    let Some(viewport) = props.viewport_ref().current() else {
        return false;
    };
    let Some(section) = props.section_refs()[target].current() else {
        return false;
    };
    let Some(scroll) = viewport.scroll() else {
        return false;
    };
    let relative_line = section
        .bounding_rect()
        .line
        .saturating_sub(viewport.content_rect().line);
    let offset = i64::from(scroll.offset.y)
        .saturating_add(i64::from(relative_line))
        .clamp(0, i64::from(scroll.max_offset.y)) as u32;
    if let Ok(mut pending) = props.pending().lock() {
        *pending = None;
    }
    if let Ok(mut active) = props.active_ref().lock() {
        *active = target;
    }
    props.set_active().set(target);
    props
        .set_scroll()
        .set(ScrollOffset::new(scroll.offset.x, offset));
    true
}

/// Computes the active section from the committed document geometry.
pub(super) fn measured_active_section(
    viewport_ref: &ElementRef,
    section_refs: &[ElementRef],
) -> Option<usize> {
    let viewport = viewport_ref.current()?;
    let scroll = viewport.scroll()?;
    if scroll.offset.y >= scroll.max_offset.y && scroll.max_offset.y > 0 {
        return section_refs.len().checked_sub(1);
    }

    let anchor = viewport.content_rect().line.saturating_add(1);
    let mut active = 0;
    for (index, section_ref) in section_refs.iter().enumerate() {
        let Some(section) = section_ref.current() else {
            continue;
        };
        if section.bounding_rect().line <= anchor {
            active = index;
        } else {
            break;
        }
    }
    Some(active)
}

/// Scroll offset that brings `section_ref` to the top of the viewport.
fn scroll_target(viewport_ref: &ElementRef, section_ref: &ElementRef) -> Option<ScrollOffset> {
    let viewport = viewport_ref.current()?;
    let section = section_ref.current()?;
    let scroll = viewport.scroll()?;
    let relative_line = section
        .bounding_rect()
        .line
        .saturating_sub(viewport.content_rect().line);
    let target = i64::from(scroll.offset.y)
        .saturating_add(i64::from(relative_line))
        .clamp(0, i64::from(scroll.max_offset.y)) as u32;
    Some(ScrollOffset::new(scroll.offset.x, target))
}

/// Breadcrumb label for a chapter, used by the header and results.
pub(crate) fn breadcrumb(meta: &ChapterMeta) -> String {
    format!("{} · {}", meta.number, meta.title)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy};
    use icmd::{Component, ComponentContext, ElementRef, Node, Props, Size};

    use super::*;
    use crate::metadata::CHAPTERS;

    #[test]
    fn scroll_target_requires_committed_refs() {
        assert!(scroll_target(&ElementRef::new(), &ElementRef::new()).is_none());
    }

    #[test]
    fn measured_active_section_requires_committed_viewport() {
        assert!(measured_active_section(&ElementRef::new(), &[]).is_none());
    }

    #[test]
    fn registry_matches_router_arity() {
        for (index, meta) in CHAPTERS.iter().enumerate() {
            assert_eq!(meta.index, index);
        }
    }

    /// A document whose pending destination is parked before anything commits.
    ///
    /// The test shares the pending cell and observes that the first commit of
    /// the new chapter clears it, which is only possible once the target
    /// section's ref published a snapshot.
    struct PendingProbe {
        pending: Arc<Mutex<Option<usize>>>,
    }

    fn pending_probe(cx: &mut ComponentContext, props: &Props<PendingProbe>) -> Node {
        let theme = cx.use_theme();
        let pending = props.data().pending.clone();
        let (chapter, set_chapter) = cx.use_state(|| 2_usize);
        let (_active, set_active) = cx.use_state(|| 0_usize);
        let (scroll, set_scroll) = cx.use_state(ScrollOffset::default);
        let active_ref = cx.use_ref(|| 0_usize);
        let viewport_ref = cx.use_element_ref();
        let section_refs = cx.use_memo(chapter, || {
            (0..3).map(|_| ElementRef::new()).collect::<Vec<_>>()
        });
        let navigator = Navigator::new(
            chapter,
            section_refs,
            viewport_ref,
            NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
        );
        let data = ChapterProps::new(navigator, scroll, true);
        let meta = metadata::chapter(chapter);
        let content = (0..3)
            .map(|index| {
                section(
                    &theme,
                    &data,
                    index,
                    &meta.sections[index],
                    icmd::ui! {
                        <view style={|style| { style.height /= Dimension::Cells(24); }} />
                    },
                )
            })
            .collect::<Node>();
        document(&data, content)
    }

    /// What a navigation probe should ask the navigator to do.
    #[derive(Clone, Copy)]
    enum NavAction {
        /// Jump to a section in the mounted chapter.
        Section(usize),
        /// Switch chapter.
        Chapter(usize),
    }

    /// Shared observations a navigation test reads after committing.
    struct NavProbe {
        action: NavAction,
        active_ref: Arc<Mutex<usize>>,
        pending: Arc<Mutex<Option<usize>>>,
        chapter_seen: Arc<Mutex<usize>>,
        active_seen: Arc<Mutex<usize>>,
        scroll_seen: Arc<Mutex<ScrollOffset>>,
    }

    fn nav_probe(cx: &mut ComponentContext, props: &Props<NavProbe>) -> Node {
        let theme = cx.use_theme();
        let (chapter, set_chapter) = cx.use_state(|| 0_usize);
        let (active, set_active) = cx.use_state(|| 0_usize);
        let (scroll, set_scroll) = cx.use_state(ScrollOffset::default);
        let active_cell = props.data().active_ref.clone();
        let pending_cell = props.data().pending.clone();
        let active_hook = cx.use_ref(move || active_cell);
        let pending_hook = cx.use_ref(move || pending_cell);
        let active_ref = active_hook.lock().expect("active cell").clone();
        let pending = pending_hook.lock().expect("pending cell").clone();
        let viewport_ref = cx.use_element_ref();
        let section_refs = cx.use_memo(chapter, || {
            (0..3).map(|_| ElementRef::new()).collect::<Vec<_>>()
        });
        let navigator = Navigator::new(
            chapter,
            section_refs,
            viewport_ref,
            NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
        );
        *props.data().chapter_seen.lock().expect("chapter cell") = chapter;
        *props.data().active_seen.lock().expect("active cell") = active;
        *props.data().scroll_seen.lock().expect("scroll cell") = scroll;

        let action = props.data().action;
        let scoped = navigator.clone();
        cx.use_mount_effect(move || match action {
            NavAction::Section(index) => scoped.goto_section(index),
            NavAction::Chapter(index) => scoped.goto_chapter(index),
        });

        let data = ChapterProps::new(navigator, scroll, true);
        let meta = metadata::chapter(chapter);
        let content = (0..3)
            .map(|index| {
                section(
                    &theme,
                    &data,
                    index,
                    &meta.sections[index],
                    icmd::ui! {
                        <view style={|style| { style.height /= Dimension::Cells(24); }} />
                    },
                )
            })
            .collect::<Node>();
        document(&data, content)
    }

    /// Runs a probe to settlement and returns the observations it wrote.
    fn run_nav_probe(action: NavAction, active: usize, pending: Option<usize>) -> NavProbe {
        let probe = NavProbe {
            action,
            active_ref: Arc::new(Mutex::new(active)),
            pending: Arc::new(Mutex::new(pending)),
            chapter_seen: Arc::new(Mutex::new(usize::MAX)),
            active_seen: Arc::new(Mutex::new(usize::MAX)),
            scroll_seen: Arc::new(Mutex::new(ScrollOffset::default())),
        };
        let (commit, _, _) = Commit::new_with_events(Size::new(80, 24));
        let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
        let input = runtime.input();
        let output = runtime.output();
        let shared = NavProbe {
            action: probe.action,
            active_ref: probe.active_ref.clone(),
            pending: probe.pending.clone(),
            chapter_seen: probe.chapter_seen.clone(),
            active_seen: probe.active_seen.clone(),
            scroll_seen: probe.scroll_seen.clone(),
        };
        input
            .send(nav_probe.apply(shared))
            .expect("the probe enters the pipeline");
        for _ in 0..3 {
            let _ = output.recv_timeout(Duration::from_secs(5));
        }
        drop(input);
        runtime.shutdown(ShutdownPolicy::default()).unwrap();
        probe
    }

    #[test]
    fn section_jump_scrolls_and_reports_a_new_active_section() {
        let probe = run_nav_probe(NavAction::Section(2), 0, None);
        assert!(
            probe.pending.lock().expect("pending cell").is_none(),
            "a same-chapter jump must resolve immediately rather than stay parked"
        );
        assert!(
            *probe.active_seen.lock().expect("active cell") >= 1,
            "the jump must move the active section away from the first one"
        );
        assert!(
            probe.scroll_seen.lock().expect("scroll cell").y > 0,
            "the jump must scroll the document rather than stay at the top"
        );
    }

    #[test]
    fn chapter_change_resets_the_active_section_and_the_pending_target() {
        let probe = run_nav_probe(NavAction::Chapter(1), 2, Some(2));
        assert_eq!(*probe.chapter_seen.lock().expect("chapter cell"), 1);
        assert_eq!(
            *probe.active_ref.lock().expect("active cell"),
            0,
            "switching chapter resets the active-section indicator"
        );
        assert!(
            probe.pending.lock().expect("pending cell").is_none(),
            "switching chapter clears a parked destination"
        );
        assert_eq!(
            *probe.scroll_seen.lock().expect("scroll cell"),
            ScrollOffset::default(),
            "switching chapter returns the document to its top"
        );
    }

    #[test]
    fn pending_section_navigation_resolves_once_refs_mount() {
        let pending = Arc::new(Mutex::new(Some(2_usize)));
        let (commit, _, _) = Commit::new_with_events(Size::new(80, 24));
        let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input
            .send(pending_probe.apply(PendingProbe {
                pending: pending.clone(),
            }))
            .expect("the document enters the pipeline");
        let first = output
            .recv_timeout(Duration::from_secs(5))
            .expect("the document commits once");
        assert!(!first.operations.is_empty());
        // Resolving the pending target queues a controlled scroll update, which
        // commits a second frame.
        let second = output.recv_timeout(Duration::from_secs(5));
        assert!(
            pending.lock().expect("pending cell is usable").is_none(),
            "the parked destination must clear once the target ref mounts"
        );
        drop(second);
        drop(input);
        runtime.shutdown(ShutdownPolicy::default()).unwrap();
    }
}
