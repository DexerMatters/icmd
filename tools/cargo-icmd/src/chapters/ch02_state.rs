//! Chapter 02 — Components and State.
//!
//! Gives a repeated piece of interface a name and a typed contract, then covers
//! the practical hook model: state, refs, memoized work, effects, and context.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use icmd::{
    Align, Attr, BadgeVariant, ButtonVariant, Component, ComponentContext, ContextKey, Dimension,
    Edges, ElementSnapshot, Justify, Layout, Node, Props, Span, Text, TextWrap, badge, button,
    card, code, column, create_context, empty, heading, muted, paragraph, row, ui, view,
};

use super::{ChapterProps, document, masthead, section};
use crate::demos;
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// Props for the chapter's own reusable status row.
#[derive(Clone, Default)]
struct StatusRowProps {
    label: Attr<String>,
    value: Attr<String>,
    variant: Attr<BadgeVariant>,
}

/// Reusable status row: a label, a value, and a badge that names its own role.
fn status_row(_cx: &mut ComponentContext, props: &Props<StatusRowProps>) -> Node {
    let label = props.data().label.clone() | String::from("unnamed");
    let value = props.data().value.clone() | String::from("—");
    let variant = props.data().variant | BadgeVariant::Muted;
    ui! {
        <card style={|style| { style.gap /= 0; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::from_spans([
                    Span::new(label).bold(),
                    Span::new("  "),
                    Span::new(value),
                ])}
                <badge text={variant_name(variant)} variant={variant} />
            </row>
        </card>
    }
}

/// Display name for a badge role; status never relies on color alone.
fn variant_name(variant: BadgeVariant) -> String {
    match variant {
        BadgeVariant::Primary => "running",
        BadgeVariant::Secondary => "ok",
        BadgeVariant::Accent => "notice",
        BadgeVariant::Muted => "idle",
        BadgeVariant::Destructive => "failed",
    }
    .to_string()
}

/// A release gate that reuses one typed row with changing values.
fn status_board_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (rechecked, set_rechecked) = cx.use_state(|| false);
    let (runs, set_runs) = cx.use_state(|| 1_u32);
    let rerun = set_rechecked.clone();
    let count = set_runs.clone();
    let build_variant = if rechecked {
        BadgeVariant::Secondary
    } else {
        BadgeVariant::Primary
    };
    let docs_variant = if rechecked {
        BadgeVariant::Secondary
    } else {
        BadgeVariant::Muted
    };
    let build_value = if rechecked { "passing" } else { "queued" };
    let docs_value = if rechecked { "up to date" } else { "waiting" };
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <status_row label="build" value={build_value} variant={build_variant} />
            <status_row label="docs" value={docs_value} variant={docs_variant} />
            <status_row label="deploy" />
            <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| {
                        rerun.update(|value| *value = !*value);
                        count.update(|value| *value += 1);
                    }}>"rerun checks"</button>
                <muted>{Text::new(format!(
                    "run {runs} · the third row passes only a label"
                ))}</muted>
            </row>
        </column>
    }
}

/// A reusable panel that wraps caller-supplied content.
fn panel(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let border = theme.colors.border;
    ui! {
        <card style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges::all(1);
            style.border.foreground /= border;
        }}>
            <heading>"Panel"</heading>
            {props.children_node()}
        </card>
    }
}

/// Mounts the same panel with two different bodies, one of them conditional.
fn children_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (expanded, set_expanded) = cx.use_state(|| true);
    let toggle = set_expanded.clone();
    let checks = ["unicode width", "cell diffing", "focus routing"];
    let list = checks
        .iter()
        .map(|name| ui! { <muted>{Text::new(format!("· {name}"))}</muted> })
        .collect::<Node>();
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>
                    {if expanded { "hide detail" } else { "show detail" }}
                </button>
                <code>"props.children_node()"</code>
            </row>
            <panel>
                <paragraph>{"The first body is prose, plus a child the caller renders only sometimes."}</paragraph>
                {if expanded {
                    ui! { <muted>{Text::new("expanded: the caller added this line")}</muted> }
                } else {
                    empty()
                }}
            </panel>
            <panel>
                {list}
                <muted>{Text::new("a collection becomes one fragment child")}</muted>
            </panel>
        </column>
    }
}

/// Props for one task row.
#[derive(Clone, Default)]
struct TaskRowProps {
    label: Attr<String>,
}

/// One task row owning local state, so key identity becomes observable.
fn task_row(cx: &mut ComponentContext, props: &Props<TaskRowProps>) -> Node {
    let task = props.data().label.clone() | String::from("untitled task");
    let (done, set_done) = cx.use_state(|| false);
    let toggle = set_done.clone();
    let theme = cx.use_theme();
    let cue = if done {
        theme.colors.secondary
    } else {
        theme.colors.muted_foreground
    };
    ui! {
        <row style={|style| {
            style.width /= Dimension::Max;
            style.align /= Align::Center;
            style.justify /= Justify::SpaceBetween;
            style.gap /= 1;
            style.padding /= Edges::symmetric(0, 1);
        }}>
            <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
                {Text::new(if done { "✓ done" } else { "○ open" }).foreground(cue).bold()}
                {Text::new(task).wrap(TextWrap::Soft)}
            </row>
            <button variant={ButtonVariant::Secondary}
                on_press={move |_| toggle.update(|value| *value = !*value)}>
                {if done { "reopen" } else { "finish" }}
            </button>
        </row>
    }
}

/// A keyed task list whose order and key policy can both change.
fn task_list_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const TASKS: [&str; 3] = ["write the guide", "commit frames", "ship the crate"];
    let (order, set_order) = cx.use_state(|| Vec::from([0_usize, 1, 2]));
    let (by_identity, set_by_identity) = cx.use_state(|| true);
    let reverse = set_order.clone();
    let flip = set_by_identity.clone();
    let rows = order
        .iter()
        .enumerate()
        .map(|(position, id)| {
            let key = if by_identity {
                *id as u64
            } else {
                position as u64
            };
            ui! { <task_row key={key} label={TASKS[*id]} /> }
        })
        .collect::<Node>();
    let policy = if by_identity {
        "key = task id"
    } else {
        "key = position"
    };
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                <muted>{Text::new(policy)}</muted>
                <row style={|style| { style.gap /= 1; }}>
                    <button on_press={move |_| flip.update(|value| *value = !*value)}>
                        "switch key policy"
                    </button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| reverse.update(|order| *order = demos::reversed(order))}>
                        "reverse"
                    </button>
                </row>
            </row>
            {rows}
            <muted>{Text::new(
                "finish a row, reverse the list, and watch which mark moves.",
            )}</muted>
        </column>
    }
}

/// A queue-depth counter that separates `set` from queued `update` calls.
fn counter_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (queued, set_queued) = cx.use_state(|| 0_i64);
    let add = set_queued.clone();
    let batch = set_queued.clone();
    let drain = set_queued;
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::from_spans([
                Span::new("queued jobs = ").foreground(primary).bold(),
                Span::new(queued.to_string()).bold(),
            ])}
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                <button on_press={move |_| add.update(|value| *value += 1)}>"enqueue one"</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| {
                        batch.update(|value| *value += 1);
                        batch.update(|value| *value += 1);
                        batch.update(|value| *value += 1);
                    }}>"enqueue three"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| drain.set(0)}>"drain (set)"</button>
            </row>
            <muted>{Text::new(
                "`update` queues work against the latest value; `set` replaces it. Three updates in one press apply in order and still paint once.",
            )
            .wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// Reads committed bounds through an element ref while two other refs stay quiet.
fn refs_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let measured = cx.use_element_ref();
    let renders = cx.use_ref(|| 0_u32);
    let last_width = Arc::new(cx.use_state_ref(|| 0_u32));
    let (report, set_report) = cx.use_state(|| String::from("waiting for the first commit"));

    let render_count = {
        let mut count = renders.lock().expect("render counter poisoned");
        *count += 1;
        *count
    };
    let observed_width = last_width.read(|width| *width).unwrap_or(0);

    let measured_for_element = measured.clone();
    let observed_for_element = Arc::clone(&last_width);
    let theme = cx.use_theme();
    ui! {
        <card element_ref={measured_for_element}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                let next = match snapshot {
                    Some(snapshot) => {
                        let bounds = snapshot.bounding_rect();
                        let _ = observed_for_element.update(|width| *width = bounds.width);
                        format!(
                            "bounds {}x{} at ({}, {})",
                            bounds.width, bounds.height, bounds.line, bounds.column
                        )
                    }
                    None => String::from("not committed"),
                };
                set_report.set(next);
            }}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges::all(1);
                style.border.foreground /= theme.colors.border;
            }}>
            {Text::new("MEASURED").bold()}
            <muted>{Text::new(report).wrap(TextWrap::Soft)}</muted>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                {Text::new(format!("renders: {render_count}"))}
                {Text::new(format!("last committed width: {observed_width} cells"))}
            </row>
            <muted>{Text::new(
                "the counter only moves when something else renders; a ref read never schedules one.",
            )
            .wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// A start/stop worker whose thread is always cancelled by its cleanup.
fn activity_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (running, set_running) = cx.use_state(|| false);
    let (ticks, set_ticks) = cx.use_state(|| 0_u32);
    let (precision, set_precision) = cx.use_state(|| 0_u8);
    let (mounted, set_mounted) = cx.use_state(|| false);
    let recomputes = cx.use_ref(|| 0_u32);

    cx.use_mount_effect(move || set_mounted.set(true));
    let counter = recomputes.clone();
    let report = cx.use_memo((ticks, precision), move || {
        let mut count = counter.lock().expect("recompute counter poisoned");
        *count += 1;
        format!("{ticks} ticks · {precision} decimal places")
    });
    cx.use_effect(running, move || {
        let stop = Arc::new(AtomicBool::new(false));
        if running {
            let worker_stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(200));
                    set_ticks.update(|value| *value += 1);
                }
            });
        }
        let cleanup_stop = Arc::clone(&stop);
        move || cleanup_stop.store(true, Ordering::Relaxed)
    });

    let recompute_count = *recomputes.lock().expect("recompute counter poisoned");
    let toggle = set_running.clone();
    let theme = cx.use_theme();
    let cue = if running {
        theme.colors.secondary
    } else {
        theme.colors.muted_foreground
    };
    let label = if running {
        "stop worker"
    } else {
        "start worker"
    };
    let mount_note = if mounted { "mounted" } else { "mounting" };
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::from_spans([
                    Span::new("worker ").bold(),
                    Span::new(if running { "running" } else { "stopped" }).foreground(cue).bold(),
                ])}
                <muted>{Text::new(mount_note)}</muted>
            </row>
            <muted>{Text::new(report).wrap(TextWrap::Soft)}</muted>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button variant={if running { ButtonVariant::Destructive } else { ButtonVariant::Primary }}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>{label}</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| set_precision.update(|value| *value = (*value + 1) % 3)}>
                    "change precision"
                </button>
                <muted>{Text::new(format!("memo recomputes: {recompute_count}"))}</muted>
            </row>
            <muted>{Text::new(
                "starting the worker spawns one thread; stopping it or unmounting the chapter cancels the thread.",
            )
            .wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// The review policy a subtree reads from context.
#[derive(Clone)]
struct ReviewPolicy {
    channel: String,
    approvals: u8,
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        Self {
            channel: String::from("stable"),
            approvals: 2,
        }
    }
}

/// The chapter-local context key, created once for the process.
fn review_policy_context() -> &'static ContextKey<ReviewPolicy> {
    static KEY: OnceLock<ContextKey<ReviewPolicy>> = OnceLock::new();
    KEY.get_or_init(|| create_context(ReviewPolicy::default()))
}

/// Reads the policy and the theme from context instead of from props.
fn policy_reader(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let policy = cx.use_context(review_policy_context);
    let accent = theme.colors.accent;
    ui! {
        <card style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.border.foreground /= theme.colors.border;
        }}>
            <muted>{Text::new("reader below the provider")}</muted>
            {Text::from_spans([
                Span::new("channel ").foreground(accent).bold(),
                Span::new(policy.channel.clone()).bold(),
                Span::new(format!("  ·  approvals {}", policy.approvals)),
            ])}
        </card>
    }
}

/// Publishes a chapter-local context value, then reads it from a descendant.
fn context_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (policy, set_policy) = cx.use_state(ReviewPolicy::default);
    let (exit_requested, set_exit_requested) = cx.use_state(|| false);
    let handle = cx.use_handle();
    let frames = handle.frames_presented();
    let canary_active = policy.channel == "canary";
    cx.provide(review_policy_context(), policy.clone());
    let stable = set_policy.clone();
    let canary = set_policy.clone();
    let request = set_exit_requested.clone();
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let exit_note = if exit_requested {
        "exit recorded locally; the guide kept running"
    } else {
        "the guide never closes itself from chapter content"
    };
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| { style.width /= Dimension::Max; style.align /= Align::Center; style.gap /= 1; }}>
                <button variant={if canary_active { ButtonVariant::Secondary } else { ButtonVariant::Primary }}
                    on_press={move |_| stable.set(ReviewPolicy { channel: String::from("stable"), approvals: 2 })}>
                    "stable policy"
                </button>
                <button variant={if canary_active { ButtonVariant::Primary } else { ButtonVariant::Secondary }}
                    on_press={move |_| canary.set(ReviewPolicy { channel: String::from("canary"), approvals: 3 })}>
                    "canary policy"
                </button>
                <muted>{Text::new(format!("session frames presented: {frames}"))}</muted>
            </row>
            <policy_reader />
            <row style={|style| { style.width /= Dimension::Max; style.align /= Align::Center; style.gap /= 1; }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| request.set(true)}>"request exit (sample)"</button>
                <muted>{Text::new(exit_note).wrap(TextWrap::Soft)}</muted>
            </row>
            <muted>{Text::new(
                "`use_theme` is the same mechanism: the active theme is just a value published above the whole tree.",
            )
            .wrap(TextWrap::Soft)}</muted>
            <row style={move |style| { style.gap /= 1; }}>
                {Text::new("primary role").foreground(primary).bold()}
                {Text::new("·").foreground(muted_foreground)}
                {Text::new("muted role").foreground(muted_foreground)}
            </row>
        </column>
    }
}

/// Chapter 02.
pub(super) fn components_and_state(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(1);
    let sections: &[SectionMeta] = meta.sections;

    let typed_props = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body_pair(
                "A component function takes a context and a typed `Props<T>` payload and returns a `Node`. When a piece of interface repeats, give it a name, a props struct, and defaults. `Props<()>` is the honest way to say a component takes no data, and `.apply(())` is how it becomes a node.",
                "Every field can stay optional through `Attr<T>`. An unset field is not an error: the component decides what an unset value means. That is what makes defaults a component decision rather than a caller obligation.",
            )}
            {docs::live_example(
                &theme,
                "typed status rows",
                "Press rerun and watch one reusable row accept fresh props; the third row passes only a label.",
                ui! { {status_board_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "Attr<T>", "unset, set, or inherited; `|` resolves it against a fallback")} }),
            )}
            {docs::source_block(&theme, "one reusable row with typed props", snippets::STATUS_ROW)}
            {docs::notice(&theme, "`Props::data()` returns the typed payload. `Props::host_props(defaults)` merges a caller's style, events, and refs over the widget's own defaults, which is how a reusable component still accepts a one-off style.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Component", purpose: "trait every component function satisfies", defaults: "—", events: "none" },
                ApiRow { name: "Props<T>", purpose: "typed payload plus DOM props and children", defaults: "Props::default()", events: "none" },
                ApiRow { name: "Attr<T>", purpose: "tri-state prop value: unset, set, or inherited", defaults: "Attr::Unset", events: "none" },
                ApiRow { name: "Props::data", purpose: "borrow the typed payload", defaults: "—", events: "none" },
                ApiRow { name: "Props::host_props", purpose: "merge caller DOM props over widget defaults", defaults: "—", events: "none" },
            ])}
        },
    );

    let children = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("Composition is how a component stays useful: it names the frame and lets the caller supply the inside. `props.children_node()` returns everything written between the tags as one node. `fragment` groups siblings without introducing a box, and `empty()` is a deliberate nothing so a conditional child can disappear without unbalancing the tree.")}
            {docs::live_example(
                &theme,
                "one panel, two bodies",
                "The same panel component receives prose in one instance and a collection in the other.",
                ui! { {children_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "a panel with a children slot", snippets::PANEL_CHILDREN)}
            {docs::notice(&theme, "Children are ordinary nodes with no wrapper of their own. A collection rendered as a child becomes a single fragment, so a list of rows needs no extra box, and a conditional that produces no content uses `empty()` rather than a zero-sized view.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "children_node", purpose: "the caller's content as one node", defaults: "empty fragment", events: "none" },
                ApiRow { name: "fragment", purpose: "group siblings without a box", defaults: "no styling", events: "none" },
                ApiRow { name: "empty", purpose: "render nothing on purpose", defaults: "no layout box", events: "none" },
                ApiRow { name: "Node", purpose: "an ordinary tree value, children included", defaults: "—", events: "none" },
            ])}
        },
    );

    let keys = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("The reconciler matches nodes across renders by type and key. Position is the default identity, which is correct for a fixed order and wrong the moment a collection is sorted, filtered, or reversed. A key states which item a row is, so local state and focus follow the item instead of the slot.")}
            {docs::live_example(
                &theme,
                "keyed task list",
                "Finish a row, then reverse. With id keys the mark travels with the task; with position keys it stays in the slot.",
                ui! { {task_list_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "keyed rows that keep their identity", snippets::TASK_LIST)}
            {docs::watch_for(&theme, "A key must be stable for the lifetime of the item. Deriving the key from the array index reproduces exactly the bug keys exist to prevent; the demo's policy switch shows what that looks like in one press.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "key", purpose: "stable item identity across renders", defaults: "positional identity", events: "none" },
                ApiRow { name: "Key", purpose: "the resolved identity a node carries", defaults: "none", events: "none" },
                ApiRow { name: "Node", purpose: "a keyed member of a collection", defaults: "—", events: "none" },
                ApiRow { name: "use_state", purpose: "row-local state that identity must preserve", defaults: "initial closure", events: "set, update" },
            ])}
        },
    );

    let visible_state = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body_pair(
                "`use_state` returns the current value and a `StateSetter`. The setter is a plain cloneable handle, not a borrow of the component: clone it into each handler, move it into worker threads, and the runtime keeps ownership of the value.",
                "Two methods express intent. `set` replaces the value, which is right for a reset or a selection. `update` receives the latest value and mutates it, which is right for counters and toggles. Updates queued during one event are applied in order before the next render.",
            )}
            {docs::live_example(
                &theme,
                "queued updates",
                "`enqueue three` calls update three times in one press; the counter jumps by three and the frame paints once.",
                ui! { {counter_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "a controlled counter", snippets::COUNTER_STATE)}
            {docs::notice(&theme, "A setter never blocks the render. It queues an update and wakes the runtime; the value read during a render is the value the previous commit produced. That is why a temporary variable, not the just-set value, is what a handler sees.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_state", purpose: "value that survives renders and schedules them", defaults: "initial closure runs once", events: "set, update" },
                ApiRow { name: "StateSetter", purpose: "cloneable update handle", defaults: "borrows nothing", events: "none" },
                ApiRow { name: "StateSetter::set", purpose: "replace the value", defaults: "—", events: "none" },
                ApiRow { name: "StateSetter::update", purpose: "mutate the latest value", defaults: "—", events: "none" },
                ApiRow { name: "ComponentContext", purpose: "the per-render hook environment", defaults: "fresh per render", events: "none" },
            ])}
        },
    );

    let refs = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("Three hooks hold values across renders, and they answer different questions. `use_ref` is a persistent cell for anything that must survive a render without causing one. `use_state_ref` is the same storage with an encapsulated lock, for shared mutation. `use_element_ref` attaches to a host element and publishes its committed geometry back to you.")}
            {docs::live_example(
                &theme,
                "refs and committed geometry",
                "The readout below is fed by an element snapshot; the render counter and the last width are pure refs.",
                ui! { {refs_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "a geometry readout", snippets::GEOMETRY_READOUT)}
            {docs::callout(&theme, CalloutKind::Info, "Reading a ref during render does not schedule a render, and writing one outside render does not wake the runtime either. Geometry is published after commit, which is why a snapshot is evidence about the last frame rather than an input to the next layout.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_ref", purpose: "persistent cell with no render scheduling", defaults: "initial closure runs once", events: "none" },
                ApiRow { name: "use_state_ref", purpose: "shared mutable value behind a lock", defaults: "read, update, try_update", events: "none" },
                ApiRow { name: "use_element_ref", purpose: "attach to a host and receive snapshots", defaults: "unattached until commit", events: "on_element_change" },
                ApiRow { name: "ElementRef", purpose: "stable handle to committed element state", defaults: "current() is None before commit", events: "none" },
                ApiRow { name: "ElementSnapshot", purpose: "rectangles, resolved style, scroll state", defaults: "replaced only when it changes", events: "none" },
            ])}
        },
    );

    let effects = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("`use_memo` caches derived work and recomputes only when its dependencies differ. `use_effect` runs after render whenever its dependencies change, and whatever it returns is the cleanup. `use_mount_effect` is `use_effect` with no dependencies, so it runs exactly once. `use_unmount` registers cleanup without running a body.")}
            {docs::live_example(
                &theme,
                "start and stop a worker",
                "Start the worker to watch ticks arrive; change precision to force the memo to recompute.",
                ui! { {activity_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "an effect-owned worker with cleanup", snippets::ACTIVITY_EFFECT)}
            {docs::watch_for(&theme, "Hooks are positional. Calling one inside a branch, a loop, or a conditionally-run closure changes the hook order between renders; the runtime will either panic on a type mismatch or hand a slot to the wrong hook.")}
            {docs::production_note(&theme, "Every worker a component starts needs an owner that can stop it. The snippet's effect returns a cleanup that flips an atomic flag, so stopping the activity and unmounting the chapter both end the thread.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_memo", purpose: "cache a derived value by dependency equality", defaults: "recompute when deps differ", events: "none" },
                ApiRow { name: "use_effect", purpose: "run work after render and return cleanup", defaults: "runs when deps change", events: "none" },
                ApiRow { name: "use_mount_effect", purpose: "effect with no dependencies, run once", defaults: "mount only", events: "none" },
                ApiRow { name: "use_unmount", purpose: "register cleanup without a body", defaults: "unmount only", events: "none" },
                ApiRow { name: "EffectResult", purpose: "`()` for no cleanup or a closure for one", defaults: "—", events: "none" },
            ])}
        },
    );

    let context = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body("Context publishes a value to a subtree, so a deep descendant can read something no intermediate component received as a prop. `create_context` makes a typed key with a default, `provide` publishes a value under it, and `use_context` reads the nearest provided value or falls back to the default. `use_theme` is the same mechanism for the active theme.")}
            {docs::live_example(
                &theme,
                "context and the session handle",
                "Switch the policy; the reader below the provider re-reads it without receiving a single prop.",
                ui! { {context_demo.apply(())} },
                None,
            )}
            {docs::notice(&theme, "A context key is process-unique. Two keys created for the same type are different values, so keep the key in a `OnceLock` or a module-level constructor and let every consumer name the same one. The default passed to `create_context` is what a consumer sees with no provider above it.")}
            {docs::callout(&theme, CalloutKind::Production, "Graceful exit is a handle, not a process signal. `use_handle` returns a cloneable `AppHandle`, and `request_exit()` asks the loop to run Unmount and Exit and restore the terminal. This sample records the request in local state instead: a chapter that closed the reader would teach exit by ambush.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "create_context", purpose: "typed key with a default value", defaults: "—", events: "none" },
                ApiRow { name: "use_context", purpose: "read the nearest provided value", defaults: "key default", events: "none" },
                ApiRow { name: "provide", purpose: "publish a value to this subtree", defaults: "—", events: "none" },
                ApiRow { name: "use_theme", purpose: "read the active theme from context", defaults: "Theme::default()", events: "none" },
                ApiRow { name: "use_handle", purpose: "cloneable session control handle", defaults: "live session handle", events: "request_exit" },
                ApiRow { name: "AppHandle", purpose: "exit request, frames, uptime", defaults: "—", events: "none" },
            ])}
        },
    );

    let background = theme.colors.background;
    document(
        data,
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.background /= background;
            }}>
                {masthead(&theme, meta, data.section_count())}
                {typed_props}
                {children}
                {keys}
                {visible_state}
                {refs}
                {effects}
                {context}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}
