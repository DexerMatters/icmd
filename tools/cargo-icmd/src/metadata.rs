//! The single documentation registry the whole guide is generated from.
//!
//! Chapters and sections carry stable ids, display numbering, editorial copy,
//! the component and type names they cover, and the keywords the search index
//! matches. The shell derives its index, breadcrumb, scroll-spy labels, and
//! searchable entries from this one table; nothing else keeps a parallel list.

/// One teaching section inside a chapter.
pub(crate) struct SectionMeta {
    /// Stable, unique identifier such as `start.first-component`.
    pub id: &'static str,
    /// Display number inside the chapter, such as `1.3`.
    pub number: &'static str,
    /// Descriptive heading.
    pub title: &'static str,
    /// Short uppercase kicker above the heading.
    pub kicker: &'static str,
    /// One-sentence promise shown under the heading.
    pub summary: &'static str,
    /// Searchable aliases and concepts that are not type names.
    pub keywords: &'static [&'static str],
    /// Component, hook, and type names this section covers.
    pub types: &'static [&'static str],
}

/// One chapter of the field guide.
pub(crate) struct ChapterMeta {
    /// Stable, unique identifier such as `start`.
    pub id: &'static str,
    /// Zero-based position, also the routing index.
    pub index: usize,
    /// Two-digit display number, such as `01`.
    pub number: &'static str,
    /// Chapter title.
    pub title: &'static str,
    /// Short uppercase kicker used by the masthead.
    pub kicker: &'static str,
    /// One-sentence description of the chapter's purpose.
    pub summary: &'static str,
    /// Searchable aliases for the chapter as a whole.
    pub keywords: &'static [&'static str],
    /// Sections in reading order.
    pub sections: &'static [SectionMeta],
}

/// Every chapter in reading order.
pub(crate) const CHAPTERS: &[ChapterMeta] = &[
    ChapterMeta {
        id: "start",
        index: 0,
        number: "01",
        title: "Start Here",
        kicker: "FOUNDATIONS",
        summary: "Move from installation to a useful mental model without assuming prior TUI experience.",
        keywords: &[
            "getting started",
            "install",
            "hello world",
            "overview",
            "quickstart",
        ],
        sections: &[
            SectionMeta {
                id: "start.belongs",
                number: "1.1",
                title: "A UI that belongs in the terminal",
                kicker: "WHY ICMD",
                summary: "Retained components, cell-aware layout, Unicode text, focus, and incremental painting in one mental model.",
                keywords: &[
                    "release card",
                    "status card",
                    "badge",
                    "divider",
                    "composition",
                ],
                types: &[
                    "card",
                    "row",
                    "heading",
                    "badge",
                    "paragraph",
                    "divider",
                    "button",
                ],
            },
            SectionMeta {
                id: "start.install",
                number: "1.2",
                title: "Install the framework",
                kicker: "SETUP",
                summary: "A pure-Rust default, the two opt-in features, and the native raster prerequisites.",
                keywords: &[
                    "cargo add",
                    "features",
                    "chafa",
                    "pkg-config",
                    "libclang",
                    "pure rust",
                ],
                types: &["RuntimeConfig", "ImageProtocol"],
            },
            SectionMeta {
                id: "start.first-component",
                number: "1.3",
                title: "Render the first component",
                kicker: "HELLO",
                summary: "A complete minimal program: imports, a component function, a `ui!` tree, and the runtime entry point.",
                keywords: &["hello world", "main", "entry point", "component", "props"],
                types: &["Component", "ComponentContext", "Props", "Node", "render"],
            },
            SectionMeta {
                id: "start.ui-syntax",
                number: "1.4",
                title: "Read the `ui!` syntax",
                kicker: "SYNTAX",
                summary: "Tags, text children, expressions, fragments, conditionals, collections, props, styles, and listeners.",
                keywords: &[
                    "macro",
                    "element syntax",
                    "children",
                    "expression",
                    "fragment",
                    "collection",
                ],
                types: &["ui", "Node", "fragment", "empty", "Attr"],
            },
            SectionMeta {
                id: "start.committed-frame",
                number: "1.5",
                title: "Understand a committed frame",
                kicker: "PIPELINE",
                summary: "Render, lower, layout, clip, commit, diff, and paint as an app-author mental model.",
                keywords: &[
                    "render", "lower", "layout", "commit", "diff", "paint", "frame",
                ],
                types: &["Node", "Frame", "ElementSnapshot"],
            },
            SectionMeta {
                id: "start.next",
                number: "1.6",
                title: "Choose the next passage",
                kicker: "ROUTES",
                summary: "Route cards for building a layout, adding state, collecting input, rendering documents, and shipping.",
                keywords: &["routes", "index", "where next", "learning path"],
                types: &["button", "card", "badge"],
            },
        ],
    },
    ChapterMeta {
        id: "state",
        index: 1,
        number: "02",
        title: "Components and State",
        kicker: "STRUCTURE",
        summary: "Reusable component boundaries and the complete practical hook model.",
        keywords: &["hooks", "state", "component", "props", "effects", "context"],
        sections: &[
            SectionMeta {
                id: "state.props",
                number: "2.1",
                title: "Component functions and typed props",
                kicker: "BOUNDARIES",
                summary: "Give a repeated piece of interface a name, a typed contract, and sensible defaults.",
                keywords: &[
                    "component function",
                    "typed props",
                    "defaults",
                    "reusable",
                    "status row",
                ],
                types: &[
                    "Component",
                    "Props",
                    "Attr",
                    "Props::data",
                    "Props::host_props",
                ],
            },
            SectionMeta {
                id: "state.children",
                number: "2.2",
                title: "Children, fragments, and composition",
                kicker: "COMPOSITION",
                summary: "Accept arbitrary content, group siblings without a wrapper box, and render nothing on purpose.",
                keywords: &[
                    "children",
                    "slot",
                    "fragment",
                    "empty",
                    "conditional children",
                    "panel",
                ],
                types: &["children_node", "fragment", "empty", "Node"],
            },
            SectionMeta {
                id: "state.keys",
                number: "2.3",
                title: "Identity and keyed collections",
                kicker: "IDENTITY",
                summary: "Keys tell the reconciler which row is which, so reordering moves state instead of destroying it.",
                keywords: &[
                    "key",
                    "list",
                    "reorder",
                    "identity",
                    "collection",
                    "reconciliation",
                ],
                types: &["Key", "Node", "row", "view"],
            },
            SectionMeta {
                id: "state.use-state",
                number: "2.4",
                title: "Visible state with `use_state`",
                kicker: "STATE",
                summary: "A controlled counter that explains setter cloning, queued updates, and `set` versus `update`.",
                keywords: &["counter", "controlled", "setter", "update", "queue"],
                types: &["use_state", "StateSetter", "ComponentContext"],
            },
            SectionMeta {
                id: "state.refs",
                number: "2.5",
                title: "Persistent values and element refs",
                kicker: "REFS",
                summary: "Distinguish `use_ref`, `use_state_ref`, and `use_element_ref`, and read committed bounds without rendering them.",
                keywords: &["ref", "element ref", "snapshot", "geometry", "persistent"],
                types: &[
                    "use_ref",
                    "use_state_ref",
                    "use_element_ref",
                    "Ref",
                    "StateRef",
                    "ElementRef",
                    "ElementSnapshot",
                ],
            },
            SectionMeta {
                id: "state.effects",
                number: "2.6",
                title: "Memoized work and effects",
                kicker: "EFFECTS",
                summary: "Dependency equality, mount effects, cleanup, and why hook order is a contract.",
                keywords: &[
                    "memo",
                    "effect",
                    "cleanup",
                    "dependencies",
                    "unmount",
                    "start stop",
                ],
                types: &[
                    "use_memo",
                    "use_effect",
                    "use_mount_effect",
                    "use_unmount",
                    "EffectResult",
                ],
            },
            SectionMeta {
                id: "state.context",
                number: "2.7",
                title: "Context and application handles",
                kicker: "SHARING",
                summary: "Publish a value to a subtree without threading it through every prop struct.",
                keywords: &["context", "provider", "theme", "handle", "exit", "graceful"],
                types: &[
                    "create_context",
                    "use_context",
                    "provide",
                    "use_theme",
                    "use_handle",
                    "AppHandle",
                ],
            },
        ],
    },
    ChapterMeta {
        id: "layout",
        index: 2,
        number: "03",
        title: "Layout and Styling",
        kicker: "GEOMETRY",
        summary: "Make terminal geometry predictable rather than trial-and-error.",
        keywords: &[
            "layout",
            "style",
            "dimension",
            "flex",
            "border",
            "responsive",
        ],
        sections: &[
            SectionMeta {
                id: "layout.boxes",
                number: "3.1",
                title: "The box vocabulary",
                kicker: "PRIMITIVES",
                summary: "Learn the small set of containers that every screen is assembled from.",
                keywords: &[
                    "box",
                    "container",
                    "column",
                    "row",
                    "section",
                    "center",
                    "spacer",
                    "footer",
                ],
                types: &[
                    "view",
                    "container",
                    "column",
                    "row",
                    "section",
                    "card",
                    "center",
                    "spacer",
                    "footer",
                ],
            },
            SectionMeta {
                id: "layout.axes",
                number: "3.2",
                title: "Main and cross axes",
                kicker: "ALIGNMENT",
                summary: "Distribution and alignment are two independent decisions on perpendicular axes.",
                keywords: &[
                    "justify",
                    "align",
                    "gap",
                    "space between",
                    "stretch",
                    "axis",
                ],
                types: &["Layout", "Justify", "Align", "gap"],
            },
            SectionMeta {
                id: "layout.sizing",
                number: "3.3",
                title: "Sizing in cells and remaining space",
                kicker: "DIMENSIONS",
                summary: "Choose between intrinsic, fixed-cell, percentage, and fill-the-rest sizing deliberately.",
                keywords: &[
                    "dimension",
                    "auto",
                    "cells",
                    "percent",
                    "max",
                    "available space",
                ],
                types: &["Dimension", "Percent", "PercentBasis"],
            },
            SectionMeta {
                id: "layout.spacing",
                number: "3.4",
                title: "Spacing, surfaces, and borders",
                kicker: "SURFACES",
                summary: "Separate siblings, pad a surface, and pick the border kind that matches the role.",
                keywords: &[
                    "margin",
                    "padding",
                    "background",
                    "fill",
                    "border",
                    "edges",
                    "rounded",
                ],
                types: &["Edges", "BorderKind", "Style", "Fill", "card"],
            },
            SectionMeta {
                id: "layout.positioning",
                number: "3.5",
                title: "Positioning and stacking",
                kicker: "OVERLAYS",
                summary: "Absolute placement and paint order put a badge exactly where flow layout cannot.",
                keywords: &[
                    "absolute",
                    "position",
                    "z index",
                    "stacking",
                    "visibility",
                    "overlay",
                    "badge",
                ],
                types: &["AxisPosition", "z_index", "Visibility", "Style"],
            },
            SectionMeta {
                id: "layout.overflow",
                number: "3.6",
                title: "Overflow and clipping",
                kicker: "BOUNDS",
                summary: "Box overflow and text overflow are separate policies, and scrolling owns clipping on its axes.",
                keywords: &["overflow", "clip", "visible", "truncate", "scroll owner"],
                types: &["Overflow", "scroll_area", "TextOverflow", "TextWrap"],
            },
            SectionMeta {
                id: "layout.measure",
                number: "3.7",
                title: "Measure committed geometry",
                kicker: "INTROSPECTION",
                summary: "Read the rectangles and resolved styles the commit stage actually produced.",
                keywords: &[
                    "element ref",
                    "snapshot",
                    "bounding rect",
                    "content rect",
                    "resolved style",
                    "scroll state",
                ],
                types: &[
                    "ElementRef",
                    "ElementSnapshot",
                    "ElementRect",
                    "ResolvedElementStyle",
                    "ElementScrollState",
                ],
            },
            SectionMeta {
                id: "layout.responsive",
                number: "3.8",
                title: "Build responsive terminal layouts",
                kicker: "RESPONSIVE",
                summary: "Use the same width breakpoints this guide uses to choose rails, columns, and controls.",
                keywords: &[
                    "breakpoint",
                    "responsive",
                    "rail",
                    "wide",
                    "narrow",
                    "resize",
                ],
                types: &["Dimension", "Visibility", "ElementRef"],
            },
        ],
    },
    ChapterMeta {
        id: "text",
        index: 3,
        number: "04",
        title: "Text and Documents",
        kicker: "PROSE",
        summary: "The framework's Unicode-aware text model and long-form content.",
        keywords: &["text", "unicode", "wrap", "markdown", "selection", "spans"],
        sections: &[
            SectionMeta {
                id: "text.components",
                number: "4.1",
                title: "Semantic text components",
                kicker: "ROLES",
                summary: "Name the role of a run of text and let the theme decide how it looks.",
                keywords: &[
                    "heading",
                    "paragraph",
                    "label",
                    "muted",
                    "code",
                    "blockquote",
                    "kbd",
                ],
                types: &[
                    "heading",
                    "paragraph",
                    "label",
                    "muted",
                    "code",
                    "blockquote",
                    "kbd",
                ],
            },
            SectionMeta {
                id: "text.spans",
                number: "4.2",
                title: "Styled spans",
                kicker: "INLINE",
                summary: "Compose one text node from independently styled runs, including the terminal attributes.",
                keywords: &[
                    "span",
                    "foreground",
                    "background",
                    "bold",
                    "italic",
                    "underline",
                ],
                types: &["Text", "Span", "TextStyle", "Attributes"],
            },
            SectionMeta {
                id: "text.unicode",
                number: "4.3",
                title: "Unicode occupies cells, not bytes",
                kicker: "WIDTHS",
                summary: "Graphemes, wide CJK cells, combining marks, and emoji all participate in alignment.",
                keywords: &[
                    "cjk",
                    "emoji",
                    "grapheme",
                    "wide cell",
                    "combining",
                    "unicode width",
                ],
                types: &["Text", "EmojiMerging", "Span"],
            },
            SectionMeta {
                id: "text.wrapping",
                number: "4.4",
                title: "Wrapping, alignment, and truncation",
                kicker: "FITTING",
                summary: "Choose how text reflows, where it sits in its box, and what happens when it still overflows.",
                keywords: &["nowrap", "soft", "hard", "ellipsis", "clip", "align"],
                types: &["TextWrap", "TextAlign", "TextOverflow", "Text"],
            },
            SectionMeta {
                id: "text.links",
                number: "4.5",
                title: "Links report intent",
                kicker: "LINKS",
                summary: "A link reports its target; opening anything stays an application decision.",
                keywords: &["link", "href", "follow", "url", "open"],
                types: &["link", "LinkProps", "on_follow"],
            },
            SectionMeta {
                id: "text.selection",
                number: "4.6",
                title: "Selectable documents",
                kicker: "SELECTION",
                summary: "Pointer drag, keyboard extension, select-all, and copy over the committed frame.",
                keywords: &["selection", "copy", "clipboard", "select all", "drag"],
                types: &[
                    "selection_area",
                    "SelectionAreaProps",
                    "TextSelectionEvent",
                    "ElementRef",
                ],
            },
            SectionMeta {
                id: "text.markdown",
                number: "4.7",
                title: "Markdown as ordinary components",
                kicker: "DOCUMENTS",
                summary: "One opt-in feature turns CommonMark into selectable native components.",
                keywords: &[
                    "markdown",
                    "commonmark",
                    "table",
                    "task list",
                    "footnote",
                    "alert",
                ],
                types: &["markdown", "MarkdownProps", "selection_area"],
            },
        ],
    },
    ChapterMeta {
        id: "controls",
        index: 4,
        number: "05",
        title: "Controls and Events",
        kicker: "INTERACTION",
        summary: "State ownership, semantic activation, editing, focus, and event flow.",
        keywords: &[
            "controls", "events", "input", "focus", "keyboard", "pointer",
        ],
        sections: &[
            SectionMeta {
                id: "controls.buttons",
                number: "5.1",
                title: "Buttons and semantic activation",
                kicker: "ACTIVATION",
                summary: "Buttons own press semantics, so a keyboard activation and a click arrive through one path.",
                keywords: &[
                    "button",
                    "press",
                    "click",
                    "disabled",
                    "autofocus",
                    "variant",
                ],
                types: &["button", "ButtonProps", "ButtonVariant", "on_press"],
            },
            SectionMeta {
                id: "controls.selection-controls",
                number: "5.2",
                title: "Checkboxes, radios, and switches",
                kicker: "CHOICES",
                summary: "Controlled selection controls report the change they want instead of mutating themselves.",
                keywords: &[
                    "checkbox",
                    "radio",
                    "switch",
                    "toggle",
                    "preferences",
                    "on_change",
                    "on_select",
                ],
                types: &[
                    "checkbox",
                    "radio",
                    "switch",
                    "CheckboxProps",
                    "RadioProps",
                    "SwitchProps",
                ],
            },
            SectionMeta {
                id: "controls.input",
                number: "5.3",
                title: "Controlled and uncontrolled input",
                kicker: "EDITING",
                summary: "Pick one owner for the value, then rely on the same control for the other model.",
                keywords: &[
                    "input",
                    "controlled",
                    "uncontrolled",
                    "placeholder",
                    "max length",
                    "submit",
                    "read only",
                ],
                types: &["input", "InputProps", "TextValueEvent", "on_submit"],
            },
            SectionMeta {
                id: "controls.textarea",
                number: "5.4",
                title: "Multiline editing",
                kicker: "MULTILINE",
                summary: "Wrapping, selection, clipboard, and a bounded height for longer text.",
                keywords: &["textarea", "multiline", "wrap", "clipboard", "height"],
                types: &[
                    "textarea",
                    "TextareaProps",
                    "TextWrap",
                    "TextClipboardEvent",
                ],
            },
            SectionMeta {
                id: "controls.raw-input",
                number: "5.5",
                title: "Build on `raw_input`",
                kicker: "EXTENSION",
                summary: "Restyle the editor host and intercept policy while keeping low-level editing out of application code.",
                keywords: &["raw input", "command field", "custom", "appearance", "mode"],
                types: &[
                    "raw_input",
                    "RawInputProps",
                    "RawInputAppearance",
                    "RawInputMode",
                ],
            },
            SectionMeta {
                id: "controls.focus",
                number: "5.6",
                title: "Focus and keyboard behavior",
                kicker: "FOCUS",
                summary: "Focusability, autofocus, activation keys, local key handling, and application shortcuts.",
                keywords: &[
                    "focus", "tab", "enter", "space", "shortcut", "modifier", "app key",
                ],
                types: &["autofocus", "on_key_down", "on_app_key", "KeyboardEvent"],
            },
            SectionMeta {
                id: "controls.events",
                number: "5.7",
                title: "Pointer, wheel, paste, resize, and terminal focus",
                kicker: "EVENT FLOW",
                summary: "The public event families and the capture, target, and bubble phases they share.",
                keywords: &[
                    "pointer",
                    "wheel",
                    "paste",
                    "resize",
                    "focus event",
                    "capture",
                    "bubble",
                ],
                types: &[
                    "PointerEvent",
                    "WheelEvent",
                    "PasteEvent",
                    "ResizeEvent",
                    "FocusEvent",
                    "TerminalFocusEvent",
                    "EventPhase",
                    "EventListener",
                ],
            },
            SectionMeta {
                id: "controls.ledger",
                number: "5.8",
                title: "A live event ledger",
                kicker: "LEDGER",
                summary: "A preferences form that records recent semantic events in a bounded, scrollable ledger.",
                keywords: &["ledger", "log", "event log", "bounded", "preferences form"],
                types: &[
                    "use_state",
                    "scroll_area",
                    "card",
                    "checkbox",
                    "radio",
                    "switch",
                ],
            },
        ],
    },
    ChapterMeta {
        id: "feedback",
        index: 5,
        number: "06",
        title: "Feedback and Canvas",
        kicker: "STATUS",
        summary: "Communicate status without introducing a separate charting abstraction.",
        keywords: &[
            "feedback", "progress", "spinner", "alert", "canvas", "chart",
        ],
        sections: &[
            SectionMeta {
                id: "feedback.badges",
                number: "6.1",
                title: "Badges as compact state",
                kicker: "BADGES",
                summary: "A short label with a semantic role, always paired with text so color is never the only cue.",
                keywords: &["badge", "tag", "label", "variant", "status"],
                types: &["badge", "BadgeProps", "BadgeVariant"],
            },
            SectionMeta {
                id: "feedback.alerts",
                number: "6.2",
                title: "Alerts with semantic severity",
                kicker: "ALERTS",
                summary: "Info, success, warning, and error share one card with a colored edge and a readable title.",
                keywords: &["alert", "info", "success", "warning", "error", "severity"],
                types: &["alert", "AlertProps", "AlertVariant"],
            },
            SectionMeta {
                id: "feedback.progress",
                number: "6.3",
                title: "Progress is caller-owned",
                kicker: "PROGRESS",
                summary: "The bar clamps and formats a value; deciding that value belongs to your state model.",
                keywords: &["progress", "bar", "value", "max", "clamp", "percentage"],
                types: &["progress_bar", "ProgressBarProps"],
            },
            SectionMeta {
                id: "feedback.spinners",
                number: "6.4",
                title: "Spinners and skeletons",
                kicker: "PLACEHOLDERS",
                summary: "Callers advance spinner frames and decide when a placeholder is better than a blank.",
                keywords: &["spinner", "skeleton", "frame", "placeholder", "loading"],
                types: &["spinner", "skeleton", "SpinnerProps", "SkeletonProps"],
            },
            SectionMeta {
                id: "feedback.publish",
                number: "6.5",
                title: "A publish-operation demonstration",
                kicker: "LIVE EXAMPLE",
                summary: "One state model drives a spinner, a progress bar, a badge, an alert, and a bounded status log.",
                keywords: &[
                    "publish",
                    "operation",
                    "timer",
                    "effect",
                    "cleanup",
                    "start pause reset",
                ],
                types: &[
                    "use_state",
                    "use_effect",
                    "use_mount_effect",
                    "spinner",
                    "progress_bar",
                    "alert",
                    "badge",
                ],
            },
            SectionMeta {
                id: "feedback.canvas",
                number: "6.6",
                title: "Draw directly with canvas",
                kicker: "CANVAS",
                summary: "Text, lines, rectangles, colors, attributes, and clipping in a fixed cell grid.",
                keywords: &["canvas", "draw", "chart", "line", "rect", "cells"],
                types: &[
                    "canvas",
                    "CanvasContext",
                    "CanvasProps",
                    "CanvasDraw",
                    "CanvasError",
                ],
            },
        ],
    },
    ChapterMeta {
        id: "media",
        index: 6,
        number: "07",
        title: "Scroll, Selection, and Raster Media",
        kicker: "INTERACTION",
        summary: "Interaction whose correctness depends on committed geometry.",
        keywords: &["scroll", "selection", "image", "raster", "sixel", "kitty"],
        sections: &[
            SectionMeta {
                id: "media.scroll",
                number: "7.1",
                title: "Scrollable regions",
                kicker: "SCROLLING",
                summary: "Vertical, horizontal, and two-axis scrolling with clipping, scrollbars, and wheel steps.",
                keywords: &["scroll", "axes", "scrollbar", "wheel", "clip"],
                types: &[
                    "scroll_area",
                    "ScrollAreaProps",
                    "ScrollAxes",
                    "ScrollbarVisibility",
                    "ScrollbarStyle",
                ],
            },
            SectionMeta {
                id: "media.offsets",
                number: "7.2",
                title: "Controlled versus runtime-owned offsets",
                kicker: "OWNERSHIP",
                summary: "Own the offset when the application must know it; let the runtime own it otherwise.",
                keywords: &[
                    "controlled offset",
                    "uncontrolled",
                    "on_scroll",
                    "readout",
                    "build log",
                ],
                types: &["ScrollOffset", "ScrollEvent", "ScrollDelta", "scroll_area"],
            },
            SectionMeta {
                id: "media.nesting",
                number: "7.3",
                title: "Scrollable nesting",
                kicker: "NESTING",
                summary: "Wheel ownership, focus, bounded heights, and why chrome belongs outside the document scroller.",
                keywords: &[
                    "nesting",
                    "nested scroll",
                    "height",
                    "chrome",
                    "wheel ownership",
                ],
                types: &["scroll_area", "Dimension", "ElementRef"],
            },
            SectionMeta {
                id: "media.selection-scroll",
                number: "7.4",
                title: "Selection inside scrolling",
                kicker: "SELECTION",
                summary: "Hit testing follows painted content, so selection stays correct after a scroll.",
                keywords: &["selection", "scroll", "hit test", "release notes", "copy"],
                types: &["selection_area", "scroll_area", "TextSelectionEvent"],
            },
            SectionMeta {
                id: "media.sources",
                number: "7.5",
                title: "Raster sources and loading",
                kicker: "SOURCES",
                summary: "Loaded versus file-backed sources, lazy and eager decoding, explicit dimensions, and resource limits.",
                keywords: &[
                    "image source",
                    "raster",
                    "decode",
                    "lazy",
                    "eager",
                    "budget",
                    "include_bytes",
                ],
                types: &[
                    "raster_image",
                    "ImageProps",
                    "ImageSource",
                    "ImageLoading",
                    "RasterImage",
                    "ResourceLimits",
                ],
            },
            SectionMeta {
                id: "media.fit",
                number: "7.6",
                title: "Fit, alignment, and render mode",
                kicker: "PLACEMENT",
                summary: "Contain, cover, and stretch; Auto, Native, and Symbols; and terminal protocol negotiation.",
                keywords: &[
                    "contain", "cover", "stretch", "align", "auto", "native", "symbols", "protocol",
                ],
                types: &[
                    "ImageFit",
                    "ImageAlign",
                    "ImageMode",
                    "ImageProtocol",
                    "raster_image",
                ],
            },
            SectionMeta {
                id: "media.shipping",
                number: "7.7",
                title: "Shipping images safely",
                kicker: "PRODUCTION",
                summary: "Cache identity, explicit sizing, error placeholders, budgets, and terminal capability variance.",
                keywords: &[
                    "cache",
                    "budget",
                    "placeholder",
                    "capability",
                    "packaging",
                    "asset",
                ],
                types: &[
                    "ImageSource",
                    "ImageSourceKey",
                    "ResourceLimits",
                    "ImageMode",
                ],
            },
        ],
    },
    ChapterMeta {
        id: "themes",
        index: 7,
        number: "08",
        title: "Themes",
        kicker: "DESIGN SYSTEM",
        summary: "The semantic design system, proven by the guide's own header controls.",
        keywords: &[
            "theme",
            "color",
            "palette",
            "preset",
            "contrast",
            "typography",
        ],
        sections: &[
            SectionMeta {
                id: "themes.roles",
                number: "8.1",
                title: "Theme roles instead of raw colors",
                kicker: "ROLES",
                summary: "Nineteen roles cover every surface, and each one carries a readable foreground partner.",
                keywords: &[
                    "role",
                    "palette",
                    "foreground",
                    "background",
                    "semantic color",
                ],
                types: &["ThemeColors", "Theme", "ThemeMode", "ThemePreset"],
            },
            SectionMeta {
                id: "themes.presets",
                number: "8.2",
                title: "Preset and mode selection",
                kicker: "PRESETS",
                summary: "Eight complete palettes crossed with light and dark, driven by the header controls.",
                keywords: &[
                    "preset",
                    "nord",
                    "dracula",
                    "solarized",
                    "latte",
                    "ocean",
                    "mode",
                ],
                types: &["ThemePreset", "ThemeMode", "ThemePreset::ALL"],
            },
            SectionMeta {
                id: "themes.tokens",
                number: "8.3",
                title: "Typography, spacing, borders, and scrollbars",
                kicker: "TOKENS",
                summary: "Derived tokens turn a palette into a consistent interface, shown as compact specimens.",
                keywords: &["typography", "spacing", "border", "scrollbar", "tokens"],
                types: &[
                    "ThemeTypography",
                    "ThemeSpacing",
                    "ThemeBorders",
                    "ScrollbarStyle",
                ],
            },
            SectionMeta {
                id: "themes.customize",
                number: "8.4",
                title: "Customize without breaking derivation",
                kicker: "CUSTOMIZE",
                summary: "Start from a preset, override deliberately, and know which substitutions re-derive dependent tokens.",
                keywords: &[
                    "customize",
                    "theme builder",
                    "override",
                    "derive",
                    "palette",
                ],
                types: &["ThemePreset::theme", "ThemeBuilder", "Theme::customize"],
            },
            SectionMeta {
                id: "themes.providers",
                number: "8.5",
                title: "Nested providers",
                kicker: "SCOPING",
                summary: "A nested provider restyles one subtree, and the guide proves it does not leak.",
                keywords: &["provider", "nested", "scope", "local theme", "preview"],
                types: &[
                    "theme_provider",
                    "ThemeProviderProps",
                    "theme_context",
                    "use_theme",
                ],
            },
            SectionMeta {
                id: "themes.contrast",
                number: "8.6",
                title: "Contrast and non-color cues",
                kicker: "ACCESSIBILITY",
                summary: "Measure readability and always pair status color with a label, glyph, or shape.",
                keywords: &["contrast", "accessibility", "readable", "status", "cue"],
                types: &["contrast_ratio", "MIN_TEXT_CONTRAST", "Theme::on_card"],
            },
        ],
    },
    ChapterMeta {
        id: "production",
        index: 8,
        number: "09",
        title: "Production",
        kicker: "OPERATIONS",
        summary: "Turn a convincing UI into a reliable terminal application.",
        keywords: &[
            "production",
            "runtime",
            "lifecycle",
            "limits",
            "testing",
            "release",
        ],
        sections: &[
            SectionMeta {
                id: "production.runtime",
                number: "9.1",
                title: "Runtime configuration",
                kicker: "CONFIG",
                summary: "Exit key, polling, terminal modes, protocols, and the limits that bound the session.",
                keywords: &[
                    "runtime config",
                    "exit key",
                    "mouse",
                    "paste",
                    "focus events",
                    "limits",
                ],
                types: &[
                    "RuntimeConfig",
                    "ImageProtocol",
                    "ImageUpdatePolicy",
                    "ResourceLimits",
                    "EmojiMerging",
                ],
            },
            SectionMeta {
                id: "production.lifecycle",
                number: "9.2",
                title: "Application lifecycle",
                kicker: "LIFECYCLE",
                summary: "Boot, Mount, Ready, Unmount, and Exit, with the hook that belongs in each phase.",
                keywords: &[
                    "lifecycle",
                    "boot",
                    "mount",
                    "ready",
                    "unmount",
                    "exit",
                    "render_with",
                ],
                types: &[
                    "AppLifecycle",
                    "AppPhase",
                    "AppSession",
                    "ExitReason",
                    "render_with",
                ],
            },
            SectionMeta {
                id: "production.exit",
                number: "9.3",
                title: "Graceful exit and terminal restoration",
                kicker: "SHUTDOWN",
                summary: "Exit reasons, cleanup ordering, and why process termination must not bypass restoration.",
                keywords: &[
                    "exit",
                    "restore",
                    "cleanup",
                    "handle",
                    "request exit",
                    "terminal state",
                ],
                types: &["AppHandle", "ExitReason", "use_handle", "use_unmount"],
            },
            SectionMeta {
                id: "production.effects",
                number: "9.4",
                title: "Effects and external work",
                kicker: "ASYNC",
                summary: "Threads, channels, cancellation, dependency changes, and unmount cleanup.",
                keywords: &[
                    "thread",
                    "channel",
                    "cancellation",
                    "effect",
                    "cleanup",
                    "worker",
                ],
                types: &[
                    "use_effect",
                    "use_mount_effect",
                    "EffectResult",
                    "StateSetter",
                ],
            },
            SectionMeta {
                id: "production.render-path",
                number: "9.5",
                title: "Keep the render path quiet",
                kicker: "PERFORMANCE",
                summary: "Stable structure, keyed collections, memoization, and bounded logs keep frames cheap.",
                keywords: &[
                    "performance",
                    "memo",
                    "key",
                    "bounded",
                    "redundant update",
                    "render",
                ],
                types: &["use_memo", "Key", "use_state", "ElementRef"],
            },
            SectionMeta {
                id: "production.limits",
                number: "9.6",
                title: "Resource limits and failure behavior",
                kicker: "LIMITS",
                summary: "Tree, output, image, and cache budgets described from the application's side.",
                keywords: &[
                    "limit",
                    "budget",
                    "tree size",
                    "output bytes",
                    "image cache",
                    "failure",
                ],
                types: &["ResourceLimits", "ImageRenderOptions", "RenderError"],
            },
            SectionMeta {
                id: "production.testing",
                number: "9.7",
                title: "Test at the right layer",
                kicker: "TESTING",
                summary: "Pure state tests, component commits, fixed viewports, event routing, and terminal smoke checks.",
                keywords: &[
                    "test",
                    "commit",
                    "viewport",
                    "event routing",
                    "reducer",
                    "pure function",
                ],
                types: &["Commit", "Lower", "Runtime", "Frame"],
            },
            SectionMeta {
                id: "production.checklist",
                number: "9.8",
                title: "Release checklist",
                kicker: "CHECKLIST",
                summary: "An interactive checklist for the last pass before a terminal application ships.",
                keywords: &[
                    "checklist",
                    "release",
                    "viewport sizes",
                    "keyboard only",
                    "packaging",
                ],
                types: &["checkbox", "use_state", "progress_bar", "button"],
            },
        ],
    },
    ChapterMeta {
        id: "api",
        index: 9,
        number: "10",
        title: "API Map",
        kicker: "REFERENCE",
        summary: "Fast lookup that points at the chapter containing each demonstration.",
        keywords: &["reference", "index", "lookup", "api", "map"],
        sections: &[
            SectionMeta {
                id: "api.layout",
                number: "10.1",
                title: "Composition and layout index",
                kicker: "LAYOUT",
                summary: "Every layout primitive, its purpose, and where it is demonstrated.",
                keywords: &["layout index", "views", "containers", "boxes"],
                types: &[
                    "view",
                    "container",
                    "column",
                    "row",
                    "section",
                    "card",
                    "center",
                    "spacer",
                    "footer",
                    "divider",
                ],
            },
            SectionMeta {
                id: "api.controls",
                number: "10.2",
                title: "Controls and feedback index",
                kicker: "WIDGETS",
                summary: "Every widget with its ownership model, important defaults, and emitted event.",
                keywords: &["widget index", "controls", "feedback", "events"],
                types: &[
                    "button",
                    "checkbox",
                    "radio",
                    "switch",
                    "input",
                    "textarea",
                    "raw_input",
                    "link",
                    "badge",
                    "alert",
                    "progress_bar",
                    "spinner",
                    "skeleton",
                    "selection_area",
                ],
            },
            SectionMeta {
                id: "api.hooks",
                number: "10.3",
                title: "Hooks and context index",
                kicker: "HOOKS",
                summary: "A selection guide for state, refs, memo, effects, context, theme, and handles.",
                keywords: &["hook index", "state ref memo effect context"],
                types: &[
                    "use_state",
                    "use_ref",
                    "use_state_ref",
                    "use_memo",
                    "use_effect",
                    "use_mount_effect",
                    "use_unmount",
                    "use_context",
                    "provide",
                    "use_theme",
                    "use_element_ref",
                    "use_handle",
                ],
            },
            SectionMeta {
                id: "api.media",
                number: "10.4",
                title: "Text, documents, and media index",
                kicker: "CONTENT",
                summary: "The type map for text, spans, selection, Markdown, canvas, and raster media.",
                keywords: &[
                    "text index",
                    "markdown index",
                    "canvas index",
                    "raster index",
                ],
                types: &[
                    "text",
                    "Text",
                    "Span",
                    "heading",
                    "paragraph",
                    "label",
                    "muted",
                    "code",
                    "blockquote",
                    "kbd",
                    "markdown",
                    "MarkdownProps",
                    "canvas",
                    "CanvasContext",
                    "raster_image",
                    "ImageProps",
                    "ImageSource",
                    "ImageFit",
                    "ImageMode",
                ],
            },
            SectionMeta {
                id: "api.runtime",
                number: "10.5",
                title: "Runtime and lifecycle index",
                kicker: "RUNTIME",
                summary: "High-level configuration and lifecycle types, and the boundary to rustdoc for pipeline work.",
                keywords: &[
                    "runtime index",
                    "lifecycle index",
                    "advanced",
                    "rustdoc",
                    "boundary",
                ],
                types: &[
                    "render",
                    "render_with",
                    "RuntimeConfig",
                    "AppLifecycle",
                    "AppPhase",
                    "AppSession",
                    "ExitReason",
                    "AppHandle",
                    "advanced",
                ],
            },
        ],
    },
];

/// Number of chapters, derived from the registry rather than hard-coded.
pub(crate) const CHAPTER_COUNT: usize = CHAPTERS.len();

/// Returns the chapter at `index`, or the first chapter for an out-of-range index.
pub(crate) fn chapter(index: usize) -> &'static ChapterMeta {
    CHAPTERS.get(index).unwrap_or(&CHAPTERS[0])
}

/// Returns the section at `section` inside chapter `index`.
pub(crate) fn section(index: usize, section: usize) -> Option<&'static SectionMeta> {
    chapter(index).sections.get(section)
}

/// Number of sections in the chapter at `index`.
pub(crate) fn section_count(index: usize) -> usize {
    chapter(index).sections.len()
}

/// Returns the first section that lists `name` among the types it covers.
///
/// The API map uses this to point every widget and type at the section that
/// demonstrates it, so the index cannot drift from the registry.
pub(crate) fn covering_section(name: &str) -> Option<&'static SectionMeta> {
    CHAPTERS
        .iter()
        .flat_map(|chapter| chapter.sections.iter())
        .find(|section| section.types.contains(&name))
}

/// Every widget exported by `icmd::widgets`.
///
/// The API-map coverage tests and the metadata tests share this list, so the
/// shipped binary does not carry it.
#[cfg(test)]
pub(crate) const WIDGETS: &[&str] = &[
    "alert",
    "badge",
    "blockquote",
    "button",
    "canvas",
    "card",
    "center",
    "checkbox",
    "code",
    "column",
    "container",
    "divider",
    "empty",
    "footer",
    "fragment",
    "heading",
    "input",
    "kbd",
    "label",
    "link",
    "muted",
    "paragraph",
    "progress_bar",
    "radio",
    "raw_input",
    "row",
    "scroll_area",
    "section",
    "selection_area",
    "skeleton",
    "spacer",
    "spinner",
    "switch",
    "text",
    "textarea",
    "view",
    "raster_image",
    "markdown",
];

/// Every practical high-level hook and type the guide documents.
pub(crate) const HIGH_LEVEL_TYPES: &[&str] = &[
    "use_state",
    "use_ref",
    "use_state_ref",
    "use_memo",
    "use_effect",
    "use_mount_effect",
    "use_unmount",
    "use_context",
    "provide",
    "use_theme",
    "use_element_ref",
    "use_handle",
    "Component",
    "ComponentContext",
    "Props",
    "Node",
    "Attr",
    "Style",
    "Edges",
    "Dimension",
    "Percent",
    "Layout",
    "Justify",
    "Align",
    "Overflow",
    "AxisPosition",
    "Visibility",
    "Text",
    "Span",
    "TextWrap",
    "TextAlign",
    "TextOverflow",
    "TextStyle",
    "Attributes",
    "ElementRef",
    "ElementSnapshot",
    "ElementRect",
    "ResolvedElementStyle",
    "ElementScrollState",
    "Theme",
    "ThemeColors",
    "ThemePreset",
    "ThemeMode",
    "ThemeBuilder",
    "ThemeTypography",
    "ThemeSpacing",
    "ThemeBorders",
    "ScrollbarStyle",
    "ScrollOffset",
    "ScrollEvent",
    "ScrollDelta",
    "ScrollAxes",
    "ScrollbarVisibility",
    "RuntimeConfig",
    "ImageProtocol",
    "ImageUpdatePolicy",
    "ResourceLimits",
    "EmojiMerging",
    "AppLifecycle",
    "AppPhase",
    "AppSession",
    "ExitReason",
    "AppHandle",
    "render",
    "render_with",
    "ImageSource",
    "ImageFit",
    "ImageMode",
    "ImageAlign",
    "ImageLoading",
    "RasterImage",
    "CanvasContext",
    "CanvasProps",
    "CanvasDraw",
    "CanvasError",
    "MarkdownProps",
    "PointerEvent",
    "WheelEvent",
    "PasteEvent",
    "ResizeEvent",
    "EventPhase",
    "EventListener",
    "KeyboardEvent",
    "FocusEvent",
    "TerminalFocusEvent",
    "StateSetter",
    "StateRef",
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn chapter_ids_and_indices_are_unique_and_ordered() {
        let mut ids = HashSet::new();
        for (position, chapter) in CHAPTERS.iter().enumerate() {
            assert_eq!(
                chapter.index, position,
                "chapter index must match its position"
            );
            assert!(
                ids.insert(chapter.id),
                "duplicate chapter id `{}`",
                chapter.id
            );
            assert_eq!(chapter.number, format!("{:02}", position + 1));
            assert!(!chapter.title.is_empty());
            assert!(!chapter.kicker.is_empty());
            assert!(!chapter.summary.is_empty());
        }
    }

    #[test]
    fn section_ids_numbers_and_copy_are_unique_and_complete() {
        let mut ids = HashSet::new();
        for chapter in CHAPTERS {
            assert!(
                !chapter.sections.is_empty(),
                "{} has no sections",
                chapter.id
            );
            for (position, section) in chapter.sections.iter().enumerate() {
                assert!(
                    ids.insert(section.id),
                    "duplicate section id `{}`",
                    section.id
                );
                assert_eq!(
                    section.number,
                    format!("{}.{}", chapter.index + 1, position + 1),
                    "section {} numbering",
                    section.id
                );
                assert!(!section.title.is_empty());
                assert!(!section.kicker.is_empty());
                assert!(!section.summary.is_empty());
            }
        }
    }

    #[test]
    fn every_widget_appears_in_the_registry() {
        let haystack: String = CHAPTERS
            .iter()
            .flat_map(|chapter| {
                chapter
                    .sections
                    .iter()
                    .flat_map(|section| {
                        section.types.iter().chain(section.keywords.iter()).copied()
                    })
                    .chain(std::iter::once(chapter.title))
                    .chain(chapter.keywords.iter().copied())
            })
            .collect::<Vec<_>>()
            .join(" ");
        for widget in WIDGETS {
            assert!(
                haystack.contains(widget),
                "widget `{widget}` is missing from the documentation registry"
            );
        }
    }

    #[test]
    fn every_documented_type_points_at_a_real_section() {
        let mut known = HashSet::new();
        for chapter in CHAPTERS {
            for section in chapter.sections {
                known.insert(section.id);
            }
        }
        for widget in WIDGETS.iter().chain(HIGH_LEVEL_TYPES.iter()) {
            let found = CHAPTERS.iter().any(|chapter| {
                chapter
                    .sections
                    .iter()
                    .any(|section| section.types.contains(widget))
            });
            assert!(
                found,
                "type `{widget}` must be listed by at least one section's `types`"
            );
        }
        assert_eq!(
            known.len(),
            CHAPTERS.iter().map(|c| c.sections.len()).sum::<usize>()
        );
    }
}
