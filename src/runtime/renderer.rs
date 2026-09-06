use crate::Component;
use crate::data::{
    Cell, CellSlot, Frame, Image, ImageError, ImageId, Operation, Rect, ScreenPosition, Size,
};
use crossbeam_channel::{Receiver, Sender};
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::style::{
    Attribute, Attributes, Color, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    DuplicateImage(ImageId),
    UnknownImage(ImageId),
    InvalidPatch { image: ImageId, error: ImageError },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateImage(id) => write!(f, "image {id:?} already exists"),
            Self::UnknownImage(id) => write!(f, "image {id:?} does not exist"),
            Self::InvalidPatch { image, error } => {
                write!(f, "invalid patch for {image:?}: {error}")
            }
        }
    }
}
impl Error for FrameError {}

#[derive(Debug, Clone)]
struct ImageNode {
    image: Image,
    position: ScreenPosition,
    level: i32,
    order: u64,
}

#[derive(Debug, Clone, Copy)]
struct Damage {
    line: i64,
    column: i64,
    width: i64,
    height: i64,
}

impl Damage {
    fn new(line: i64, column: i64, width: usize, height: usize) -> Self {
        Self {
            line,
            column,
            width: width as i64,
            height: height as i64,
        }
    }
    fn union(self, other: Self) -> Self {
        let left = self.column.min(other.column);
        let top = self.line.min(other.line);
        let right = (self.column + self.width).max(other.column + other.width);
        let bottom = (self.line + self.height).max(other.line + other.height);
        Self {
            line: top,
            column: left,
            width: right - left,
            height: bottom - top,
        }
    }
}

/// The retained scene and deterministic incremental renderer.
pub struct Renderer {
    viewport: Size,
    images: HashMap<ImageId, ImageNode>,
    next_order: u64,
    last: Vec<CellSlot>,
    damage: Vec<Damage>,
    full_redraw: bool,
}

impl Renderer {
    pub fn new(viewport: Size) -> Self {
        let cells =
            vec![CellSlot::Lead(Cell::blank()); viewport.width as usize * viewport.height as usize];
        Self {
            viewport,
            images: HashMap::new(),
            next_order: 0,
            last: cells,
            damage: Vec::new(),
            full_redraw: true,
        }
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    pub fn apply_frame(&mut self, frame: Frame) -> Result<(), FrameError> {
        self.validate_frame(&frame)?;
        if let Some(viewport) = frame.viewport
            && viewport != self.viewport
        {
            self.viewport = viewport;
            self.last = vec![
                CellSlot::Lead(Cell::blank());
                viewport.width as usize * viewport.height as usize
            ];
            self.damage.clear();
            self.full_redraw = true;
        }
        if frame.force_redraw {
            self.full_redraw = true;
        }

        for operation in frame.operations {
            match operation {
                Operation::Create {
                    id,
                    image,
                    position,
                    level,
                } => {
                    self.add_damage(self.node_damage(&ImageNode {
                        image: image.clone(),
                        position,
                        level,
                        order: self.next_order,
                    }));
                    self.images.insert(
                        id,
                        ImageNode {
                            image,
                            position,
                            level,
                            order: self.next_order,
                        },
                    );
                    self.next_order = self.next_order.wrapping_add(1);
                }
                Operation::Remove { id } => {
                    if let Some(node) = self.images.remove(&id) {
                        self.add_damage(self.node_damage(&node));
                    }
                }
                Operation::Move { id, position } => {
                    if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.position = position;
                        let new = Self::node_damage_for(node);
                        self.add_damage(old.union(new));
                    }
                }
                Operation::SetLevel { id, level } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let damage = Self::node_damage_for(node);
                        node.level = level;
                        damage
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
                Operation::SetOrder { id, order } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let damage = Self::node_damage_for(node);
                        node.order = order;
                        damage
                    });
                    self.next_order = self.next_order.max(order.saturating_add(1));
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
                Operation::Replace { id, image } => {
                    if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.image = image;
                        let new = Self::node_damage_for(node);
                        self.add_damage(old.union(new));
                    }
                }
                Operation::PatchRect { id, rect, rows } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let local = node
                            .image
                            .patch_rect(rect, &rows)
                            .expect("frame validation guarantees patch validity");
                        Self::node_damage_for_rect(node, local)
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
                Operation::PatchCells { id, edits } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let local = node
                            .image
                            .patch_cells(&edits)
                            .expect("frame validation guarantees patch validity");
                        Self::node_damage_for_rect(node, local)
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_frame(&self, frame: &Frame) -> Result<(), FrameError> {
        let mut dimensions: HashMap<ImageId, (usize, usize)> = self
            .images
            .iter()
            .map(|(id, node)| (*id, (node.image.width(), node.image.height())))
            .collect();
        for operation in &frame.operations {
            match operation {
                Operation::Create { id, image, .. } => {
                    if dimensions
                        .insert(*id, (image.width(), image.height()))
                        .is_some()
                    {
                        return Err(FrameError::DuplicateImage(*id));
                    }
                }
                Operation::Remove { id } => {
                    if dimensions.remove(id).is_none() {
                        return Err(FrameError::UnknownImage(*id));
                    }
                }
                Operation::Move { id, .. }
                | Operation::SetLevel { id, .. }
                | Operation::SetOrder { id, .. } => {
                    if !dimensions.contains_key(id) {
                        return Err(FrameError::UnknownImage(*id));
                    }
                }
                Operation::Replace { id, image } => {
                    if !dimensions.contains_key(id) {
                        return Err(FrameError::UnknownImage(*id));
                    }
                    dimensions.insert(*id, (image.width(), image.height()));
                }
                Operation::PatchRect { id, rect, rows } => {
                    let Some((width, height)) = dimensions.get(id) else {
                        return Err(FrameError::UnknownImage(*id));
                    };
                    if rect.width == 0
                        || rect.height == 0
                        || rect.right() > *width
                        || rect.bottom() > *height
                        || rows.len() != rect.height
                        || rows
                            .iter()
                            .any(|row| row.iter().map(Cell::width).sum::<usize>() != rect.width)
                    {
                        return Err(FrameError::InvalidPatch {
                            image: *id,
                            error: ImageError::RowWidthMismatch,
                        });
                    }
                }
                Operation::PatchCells { id, edits } => {
                    let Some((width, height)) = dimensions.get(id) else {
                        return Err(FrameError::UnknownImage(*id));
                    };
                    if edits.iter().any(|edit| {
                        edit.position.column >= *width
                            || edit.position.line >= *height
                            || (edit.cell.width() == 2 && edit.position.column + 1 >= *width)
                    }) {
                        return Err(FrameError::InvalidPatch {
                            image: *id,
                            error: ImageError::OutOfBounds,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn node_damage_for(node: &ImageNode) -> Damage {
        Damage::new(
            node.position.line as i64,
            node.position.column as i64,
            node.image.width(),
            node.image.height(),
        )
    }
    fn node_damage(&self, node: &ImageNode) -> Damage {
        Self::node_damage_for(node)
    }
    fn node_damage_for_rect(node: &ImageNode, rect: Rect) -> Damage {
        Damage::new(
            node.position.line as i64 + rect.line as i64,
            node.position.column as i64 + rect.column as i64,
            rect.width,
            rect.height,
        )
    }
    fn add_damage(&mut self, damage: Damage) {
        if damage.width > 0 && damage.height > 0 {
            self.damage.push(damage);
        }
    }

    fn paint_at(&self, line: usize, column: usize) -> (Option<ImageId>, CellSlot) {
        let mut winner: Option<(ImageId, &ImageNode)> = None;
        for (id, node) in &self.images {
            let local_line = line as i64 - node.position.line as i64;
            let local_column = column as i64 - node.position.column as i64;
            if local_line < 0
                || local_column < 0
                || local_line >= node.image.height() as i64
                || local_column >= node.image.width() as i64
            {
                continue;
            }
            let better = winner.as_ref().is_none_or(|(_, current)| {
                (node.level, node.order) > (current.level, current.order)
            });
            if better {
                winner = Some((*id, node));
            }
        }
        let Some((id, node)) = winner else {
            return (None, CellSlot::Lead(Cell::blank()));
        };
        let slot = node.image.cell_at(
            (line as i64 - node.position.line as i64) as usize,
            (column as i64 - node.position.column as i64) as usize,
        );
        let screen = match slot {
            CellSlot::Lead(cell) => CellSlot::Lead(cell.clone()),
            CellSlot::Continuation(cell) => CellSlot::Continuation(cell.clone()),
        };
        (Some(id), screen)
    }

    fn composed_at(&self, line: usize, column: usize) -> CellSlot {
        let (source, mut cell) = self.paint_at(line, column);
        match &cell {
            CellSlot::Lead(value) if value.width() == 2 => {
                let valid = column + 1 < self.viewport.width as usize
                    && self.paint_at(line, column + 1).0 == source
                    && matches!(self.paint_at(line, column + 1).1, CellSlot::Continuation(_));
                if !valid {
                    cell = CellSlot::Lead(value.as_blank());
                }
            }
            CellSlot::Continuation(value) => {
                let valid = column > 0
                    && self.paint_at(line, column - 1).0 == source
                    && matches!(self.paint_at(line, column - 1).1, CellSlot::Lead(previous) if previous.width() == 2);
                if !valid {
                    cell = CellSlot::Lead(value.clone());
                }
            }
            _ => {}
        }
        cell
    }

    pub fn render_diff(&mut self) -> Option<String> {
        if !self.full_redraw && self.damage.is_empty() {
            return None;
        }
        let width = self.viewport.width as usize;
        let height = self.viewport.height as usize;
        let mut desired = self.last.clone();
        if self.full_redraw {
            for line in 0..height {
                for column in 0..width {
                    desired[line * width + column] = self.composed_at(line, column);
                }
            }
        } else {
            for damage in self.damage.clone() {
                let left = damage.column.saturating_sub(2).max(0) as usize;
                let top = damage.line.saturating_sub(2).max(0) as usize;
                let right = (damage.column + damage.width + 2).clamp(0, width as i64) as usize;
                let bottom = (damage.line + damage.height + 2).clamp(0, height as i64) as usize;
                for line in top..bottom {
                    for column in left..right {
                        desired[line * width + column] = self.composed_at(line, column);
                    }
                }
            }
        }
        let ansi = encode_diff(&self.last, &desired, self.viewport, self.full_redraw);
        self.last = desired;
        self.damage.clear();
        self.full_redraw = false;
        ansi
    }
}

fn mark_footprint(changed: &mut [bool], index: usize, cell: &CellSlot) {
    if index >= changed.len() {
        return;
    }
    changed[index] = true;
    if let CellSlot::Lead(value) = cell
        && value.width() == 2
        && index + 1 < changed.len()
    {
        changed[index + 1] = true;
    }
    if matches!(cell, CellSlot::Continuation(_)) && index > 0 {
        changed[index - 1] = true;
    }
}

fn encode_diff(
    old: &[CellSlot],
    desired: &[CellSlot],
    viewport: Size,
    full: bool,
) -> Option<String> {
    let width = viewport.width as usize;
    let height = viewport.height as usize;
    let mut changed = vec![full; desired.len()];
    if !full {
        for index in 0..desired.len() {
            if old[index] != desired[index] {
                mark_footprint(&mut changed, index, &old[index]);
                mark_footprint(&mut changed, index, &desired[index]);
            }
        }
    }
    if !full && !changed.iter().any(|value| *value) {
        return None;
    }
    let mut output = String::new();
    write!(
        output,
        "{}{}{}{}",
        SavePosition,
        ResetColor,
        SetAttribute(Attribute::Reset),
        if full {
            Clear(ClearType::All).to_string()
        } else {
            String::new()
        }
    )
    .unwrap();
    let mut style = (Color::Reset, Color::Reset, Attributes::default());
    for line in 0..height {
        let mut column = 0;
        while column < width {
            let index = line * width + column;
            if !changed[index] {
                column += 1;
                continue;
            }
            if column > 0
                && matches!(desired[index - 1], CellSlot::Lead(ref cell) if cell.width() == 2)
            {
                column += 1;
                continue;
            }
            let cell = desired[index].cell();
            if !full || !desired[index].is_default() {
                if style.2 != cell.attributes {
                    write!(output, "{}", SetAttribute(Attribute::Reset)).unwrap();
                    write!(
                        output,
                        "{}{}",
                        SetForegroundColor(cell.foreground),
                        SetBackgroundColor(cell.background)
                    )
                    .unwrap();
                    for attribute in Attribute::iterator() {
                        if cell.attributes.has(attribute) {
                            write!(output, "{}", SetAttribute(attribute)).unwrap();
                        }
                    }
                    style = (cell.foreground, cell.background, cell.attributes);
                } else {
                    if style.0 != cell.foreground {
                        write!(output, "{}", SetForegroundColor(cell.foreground)).unwrap();
                        style.0 = cell.foreground;
                    }
                    if style.1 != cell.background {
                        write!(output, "{}", SetBackgroundColor(cell.background)).unwrap();
                        style.1 = cell.background;
                    }
                }
                write!(
                    output,
                    "{}{}",
                    MoveTo(column as u16, line as u16),
                    cell.symbol
                )
                .unwrap();
            }
            column += cell.width();
            if cell.width() == 0 {
                column += 1;
            }
        }
    }
    write!(
        output,
        "{}{}{}",
        ResetColor,
        SetAttribute(Attribute::Reset),
        RestorePosition
    )
    .unwrap();
    Some(output)
}

impl Component for Renderer {
    type Input = Frame;
    type Output = Result<String, FrameError>;

    fn run(mut self, input: Receiver<Self::Input>, output: Sender<Self::Output>) {
        while let Ok(first) = input.recv() {
            let mut frames = vec![first];
            for _ in 1..64 {
                match input.try_recv() {
                    Ok(frame) => frames.push(frame),
                    Err(_) => break,
                }
            }

            for frame in frames {
                if let Err(error) = self.apply_frame(frame)
                    && output.send(Err(error)).is_err()
                {
                    return;
                }
            }
            if let Some(diff) = self.render_diff()
                && output.send(Ok(diff)).is_err()
            {
                return;
            }
        }

        if let Some(diff) = self.render_diff() {
            let _ = output.send(Ok(diff));
        }
    }
}
