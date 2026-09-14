// Terminal diff encoding: the changed-cell scan and the ANSI run writer.
// It owns output assembly and knows nothing about scene retention.
#![allow(unused_imports)]

use super::*;

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

fn unreliable_advancement(cell: &Cell, merging: EmojiMerging) -> bool {
    merging.merges()
        && cell
            .symbol()
            .chars()
            .any(crate::data::is_emoji_sequence_mark)
}

#[allow(clippy::too_many_arguments)]
pub fn encode_diff(
    old: &[CellSlot],
    desired: &[CellSlot],
    viewport: Size,
    damage_rows: &[Vec<Range<usize>>],
    full: bool,
    merging: EmojiMerging,
    changed: &mut Vec<bool>,
    cells_examined: &mut u64,
) -> Result<Option<String>, FrameError> {
    let width = viewport.width as usize;
    let height = viewport.height as usize;
    if changed.capacity() < desired.len()
        && changed
            .try_reserve_exact(desired.len() - changed.capacity())
            .is_err()
    {
        return Err(FrameError::AllocationFailed {
            cells: desired.len(),
        });
    }
    if full {
        changed.clear();
        changed.resize(desired.len(), true);
        *cells_examined = cells_examined.saturating_add(desired.len() as u64);
    } else {
        // Clear and re-scan only the rows this frame touches. A one-cell patch
        // therefore examines a handful of cells, not the whole viewport.
        if changed.len() != desired.len() {
            changed.clear();
            changed.resize(desired.len(), false);
        } else {
            for (line, spans) in damage_rows.iter().enumerate() {
                if spans.is_empty() {
                    continue;
                }
                let start = line * width;
                let end = (start + width).min(desired.len());
                if start < end {
                    changed[start..end].fill(false);
                }
            }
        }
        let mut any = false;
        for (line, spans) in damage_rows.iter().enumerate() {
            for span in spans {
                for column in span.clone() {
                    let index = line * width + column;
                    if index >= desired.len() {
                        continue;
                    }
                    *cells_examined = cells_examined.saturating_add(1);
                    if old[index] != desired[index] {
                        mark_footprint(changed, index, &old[index]);
                        mark_footprint(changed, index, &desired[index]);
                        any = true;
                    }
                }
            }
        }
        if !any {
            return Ok(None);
        }
    }
    let changed_count = changed.iter().filter(|value| **value).count();
    let mut output = String::new();
    output
        .try_reserve(changed_count.saturating_mul(8).saturating_add(64))
        .map_err(|_| FrameError::AllocationFailed {
            cells: desired.len(),
        })?;
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
    let mut isolate_next_row = false;
    for line in 0..height {
        let mut column = 0;
        let row_start = line * width;
        let row_end = row_start.saturating_add(width).min(desired.len());
        let row_has_unreliable = desired[row_start..row_end].iter().any(
            |slot| matches!(slot, CellSlot::Lead(cell) if unreliable_advancement(cell, merging)),
        );
        let isolate_row = row_has_unreliable || isolate_next_row;
        // Isolate every write on rows containing (or immediately following)
        // an emoji sequence. This is a deliberately narrow compatibility path:
        // terminals disagree on cursor advancement for these graphemes, while
        // ordinary rows retain the allocation-free contiguous-run encoder.
        // Plain CJK wide cells and single-codepoint emoji stay batched.
        while column < width {
            let index = line * width + column;
            let can_write = changed[index] && (!full || !desired[index].is_default());
            if !can_write
                || (column > 0
                    && matches!(desired[index - 1], CellSlot::Lead(ref cell) if cell.width() == 2))
            {
                column += 1;
                continue;
            }
            write!(output, "{}", MoveTo(column as u16, line as u16)).unwrap();
            let run_start = column;
            while column < width {
                let index = line * width + column;
                if !changed[index]
                    || (full && desired[index].is_default())
                    || (column > 0
                        && matches!(desired[index - 1], CellSlot::Lead(ref cell) if cell.width() == 2))
                {
                    break;
                }
                let cell = desired[index].cell();
                let cell_width = cell.width().max(1);
                let cell_unreliable = unreliable_advancement(cell, merging);
                // Some terminals apply an emoji sequence's width while
                // consuming the preceding write, so rows around one are
                // addressed one cell at a time below.
                if isolate_row && column > run_start {
                    break;
                }
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
                write!(output, "{}", cell.symbol).unwrap();
                column += cell_width;
                // Emoji sequences are the class of grapheme for which
                // terminal cursor advancement still differs across emulators.
                // End the run after one so the following cell is addressed
                // with an explicit cursor position instead of inheriting a
                // potentially drifted cursor. Ordinary CJK wide cells stay
                // batched, keeping ANSI size and throughput unchanged.
                if isolate_row || cell_unreliable {
                    break;
                }
            }
        }
        isolate_next_row = row_has_unreliable;
    }
    write!(
        output,
        "{}{}{}",
        ResetColor,
        SetAttribute(Attribute::Reset),
        RestorePosition
    )
    .unwrap();
    Ok(Some(output))
}
