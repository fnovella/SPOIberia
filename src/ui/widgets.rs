use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{
        List, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState,
    },
    Frame,
};
use std::cell::Cell;

pub fn sync_scroll_offset(
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    viewport_rows: usize,
) -> (usize, bool) {
    if total_items == 0 || viewport_rows == 0 || total_items <= viewport_rows {
        offset_cell.set(0);
        return (0, false);
    }
    let selected = selected_idx.min(total_items - 1);
    let mut offset = offset_cell.get();
    if selected < offset {
        offset = selected;
    } else if selected >= offset + viewport_rows {
        offset = selected + 1 - viewport_rows;
    }
    let max_offset = total_items.saturating_sub(viewport_rows);
    offset = offset.min(max_offset);
    offset_cell.set(offset);
    (offset, true)
}

#[allow(clippy::too_many_arguments)]
fn render_vertical_scrollbar(
    f: &mut Frame<'_>,
    area: Rect,
    top_inset: u16,
    bottom_inset: u16,
    total_items: usize,
    viewport_rows: usize,
    offset: usize,
    is_focused: bool,
) {
    let height = area
        .height
        .saturating_sub(top_inset.saturating_add(bottom_inset));
    if area.width == 0 || height < 2 {
        return;
    }
    let sb_area = Rect {
        x: area.x,
        y: area.y.saturating_add(top_inset),
        width: area.width,
        height,
    };
    let color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .style(Style::default().fg(color));
    let mut sb_state = ScrollbarState::new(
        total_items.saturating_sub(viewport_rows).saturating_add(1),
    )
    .position(offset)
    .viewport_content_length(viewport_rows);
    f.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
}

pub(super) fn render_scrollable_list(
    f: &mut Frame<'_>,
    list: List<'_>,
    area: Rect,
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    is_focused: bool,
) {
    let viewport_rows = area.height.saturating_sub(2) as usize;
    let (offset, show_scrollbar) =
        sync_scroll_offset(offset_cell, total_items, selected_idx, viewport_rows);
    let selected = if total_items > 0 {
        Some(selected_idx.min(total_items - 1))
    } else {
        None
    };
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(selected);
    f.render_stateful_widget(list, area, &mut state);
    if show_scrollbar {
        render_vertical_scrollbar(
            f,
            area,
            1,
            1,
            total_items,
            viewport_rows,
            offset,
            is_focused,
        );
    }
}

pub(super) fn render_scrollable_table(
    f: &mut Frame<'_>,
    table: Table<'_>,
    area: Rect,
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    is_focused: bool,
) {
    let viewport_rows = area.height.saturating_sub(4) as usize;
    let (offset, show_scrollbar) =
        sync_scroll_offset(offset_cell, total_items, selected_idx, viewport_rows);
    let selected = if total_items > 0 {
        Some(selected_idx.min(total_items - 1))
    } else {
        None
    };
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(selected);
    f.render_stateful_widget(table, area, &mut state);
    if show_scrollbar {
        render_vertical_scrollbar(
            f,
            area,
            3,
            1,
            total_items,
            viewport_rows,
            offset,
            is_focused,
        );
    }
}
