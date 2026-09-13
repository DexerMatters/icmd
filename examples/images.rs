#![recursion_limit = "512"]

//! A small, scrollable masonry gallery for exercising the `<image>` component.
//!
//! Run with `cargo run --example images`. Native image-capable terminals use the
//! best protocol Chafa detects; every other terminal gets its symbol fallback.

use std::{error::Error, path::PathBuf, sync::OnceLock};

use icmd::{
    Align, Component, ComponentContext, Dimension, Edges, ImageFit, ImageSource, Layout, Node,
    Percent, Props, RuntimeConfig, ScrollAxes, ScrollbarVisibility, Text, render, ui,
};
use icmd::{
    badge, card, column, container, heading, label, muted, raster_image, row, scroll_area, view,
};

struct Pin {
    image: ImageSource,
    title: &'static str,
    author: &'static str,
    height: u16,
}

const PIN_DATA: [(&str, &str, &str, u16); 9] = [
    ("amber", "Sunday corner", "Mara", 13),
    ("bloom", "Little greenhouse", "Noah", 11),
    ("coast", "Salt air", "Iris", 15),
    ("desk", "Slow morning", "Theo", 10),
    ("flowers", "Market flowers", "Ari", 14),
    ("golden", "Golden hour", "Mika", 11),
    ("home", "A room to linger", "June", 14),
    ("morning", "Off the map", "Emi", 12),
    ("road", "The long way home", "Sam", 13),
];

fn pins() -> &'static [Pin] {
    static PINS: OnceLock<Vec<Pin>> = OnceLock::new();
    PINS.get_or_init(|| {
        PIN_DATA
            .iter()
            .map(|(name, title, author, height)| {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("examples/res")
                    .join(format!("{name}.jpg"));
                Pin {
                    image: ImageSource::file(path),
                    title,
                    author,
                    height: *height,
                }
            })
            .collect()
    })
}

fn pin_card(pin: &Pin) -> Node {
    let source = pin.image.clone();
    ui! {
        <card style={|s| {
            s.width /= Dimension::Cells(25);
            s.gap /= 0;
        }}>
            <raster_image src={source} width={21} height={pin.height} fit={ImageFit::Cover} />
            <label>{pin.title}</label>
            <muted>{format!("{}  ·  save", pin.author)}</muted>
        </card>
    }
}

fn gallery_column(indexes: &[usize]) -> Node {
    let items = pins();
    ui! {
        <column style={|s| {
            s.width /= Dimension::Percent(Percent::available(32));
            s.gap /= 1;
        }}>
            {indexes.iter().map(|&index| pin_card(&items[index])).collect::<Node>()}
        </column>
    }
}

fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let gallery = ui! {
        <scroll_area
            axes={ScrollAxes::Vertical}
            scrollbar_visibility={ScrollbarVisibility::Always}
            style={|s| {
                s.width /= Dimension::Max;
                s.height /= Dimension::Max;
            }}
        >
            <view style={|s| {
                s.layout /= Layout::Horizontal;
                s.width /= Dimension::Max;
                s.align /= Align::Start;
                s.gap /= 1;
            }}>
                {gallery_column(&[0, 3, 6])}
                {gallery_column(&[1, 4, 7])}
                {gallery_column(&[2, 5, 8])}
            </view>
        </scroll_area>
    };

    ui! {
        <container style={|s| {
            s.padding /= Edges::all(1);
            s.gap /= 1;
        }}>
            <view style={|s| {
                s.layout /= Layout::Horizontal;
                s.width /= Dimension::Max;
                s.align /= Align::Center;
                s.gap /= 1;
            }}>
                <heading>{Text::new("Pinboard").bold()}</heading>
                <badge text="FOR YOU" />
                <muted>"A masonry image gallery"</muted>
            </view>
            <row><label>"Home"</label><muted>"Nature"</muted><muted>"Interiors"</muted><muted>"Weekend"</muted></row>
            {gallery}
            <muted>"Scroll with the wheel, arrows, PgUp/PgDn, or the scrollbar  ·  Ctrl+C quits"</muted>
        </container>
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    render(app.apply(()), RuntimeConfig::default())?;
    Ok(())
}
