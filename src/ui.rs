use ratatui::{
  backend::Backend,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Color, Modifier, Style},
  text::Spans,
  widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table},
  Terminal,
};

use crate::{App, AppMode};

/// Computes a centered rectangle within the given area.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
  let popup_layout = Layout::default()
      .direction(Direction::Vertical)
      .constraints(
          [
              Constraint::Percentage((100 - percent_y) / 2),
              Constraint::Percentage(percent_y),
              Constraint::Percentage((100 - percent_y) / 2),
          ]
          .as_ref(),
      )
      .split(r);
  Layout::default()
      .direction(Direction::Horizontal)
      .constraints(
          [
              Constraint::Percentage((100 - percent_x) / 2),
              Constraint::Percentage(percent_x),
              Constraint::Percentage((100 - percent_x) / 2),
          ]
          .as_ref(),
      )
      .split(popup_layout[1])[1]
}

pub fn draw_app<B: Backend>(
  terminal: &mut Terminal<B>,
  app: &App,
  mode: &AppMode,
  spinner_chars: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
  terminal.draw(|f| {
      let size = f.size();
      // Outer layout: main area and bottom legend.
      let outer_chunks = Layout::default()
          .direction(Direction::Vertical)
          .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
          .split(size);
      // Main area: left panel (33%) and right panel (67%).
      let main_chunks = Layout::default()
          .direction(Direction::Horizontal)
          .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
          .split(outer_chunks[0]);
      // Left panel: top for device list, bottom for device details.
      let left_chunks = Layout::default()
          .direction(Direction::Vertical)
          .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
          .split(main_chunks[0]);

      // Build device list items.
      let items: Vec<ListItem> = app.devices.iter().enumerate().map(|(i, dev)| {
          let mut text = dev.name.clone();
          if let AppMode::Scanning { device_index, spinner_index } = mode {
              if i == *device_index {
                  text = format!("{} {}", dev.name, spinner_chars[*spinner_index]);
              }
          } else if dev.ejectable {
              text = format!("{} ⏏", dev.name);
          }
          ListItem::new(Spans::from(text))
      }).collect();

      let list = List::new(items)
          .block(Block::default().borders(Borders::ALL).title("Devices"))
          .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
          .highlight_symbol(">> ");
      let mut list_state = ListState::default();
      list_state.select(Some(app.selected));
      f.render_stateful_widget(list, left_chunks[0], &mut list_state);

      // Device details panel.
      let device_details = if !app.devices.is_empty() {
          let device = &app.devices[app.selected];
          let total_gb = device.total_space as f64 / 1024_f64.powi(3);
          let free_gb = device.available_space as f64 / 1024_f64.powi(3);
          let mut info = format!(
              "Name: {}\nMount: {}\nTotal: {:.2} GB\nFree: {:.2} GB",
              device.name, device.mount_point, total_gb, free_gb
          );
          if let Some(extra) = &device.vendor_info {
              info.push_str("\nInfo:");
              for part in extra.split(',') {
                  info.push_str(&format!("\n       - {}", part.trim()));
              }
          }
          info
      } else {
          "No devices found.".to_string()
      };
      let details_paragraph = Paragraph::new(device_details)
          .block(Block::default().borders(Borders::ALL).title("Device Details"));
      f.render_widget(details_paragraph, left_chunks[1]);

      // Right panel: depends on mode.
      match mode {
          AppMode::Normal => {
              let right_panel = Paragraph::new("Empty panel")
                  .block(Block::default().borders(Borders::ALL).title("Right Panel"));
              f.render_widget(right_panel, main_chunks[1]);
          },
          AppMode::Scanning { .. } => {
              let scan_panel = Paragraph::new("Scanning in progress...")
                  .block(Block::default().borders(Borders::ALL).title("Scan Status"));
              f.render_widget(scan_panel, main_chunks[1]);
          },
          AppMode::FileList { file_entries, selected: _ } => {
              let rows: Vec<Row> = file_entries.iter().map(|entry| {
                  let size_str = format!("{} bytes", entry.size);
                  Row::new(vec![entry.name.clone(), entry.path.clone(), size_str])
              }).collect();
              let table = Table::new(rows)
                  .header(
                      Row::new(vec!["Name", "Path", "File Size"])
                          .style(Style::default().fg(Color::LightBlue))
                          .bottom_margin(1),
                  )
                  .block(Block::default().borders(Borders::ALL).title("Files"))
                  .widths(&[
                      Constraint::Percentage(30),
                      Constraint::Percentage(50),
                      Constraint::Percentage(20),
                  ]);
              f.render_widget(table, main_chunks[1]);
          },
          _ => {
              let right_panel = Paragraph::new("Empty panel")
                  .block(Block::default().borders(Borders::ALL).title("Right Panel"));
              f.render_widget(right_panel, main_chunks[1]);
          }
      }

      // Bottom legend.
      let legend_text = "Keys: j = next, k = previous, e = eject, s = scan, r = refresh, q = quit, b = back (from file list)";
      let legend = Paragraph::new(legend_text)
          .block(Block::default().borders(Borders::ALL).title("Legend"));
      f.render_widget(legend, outer_chunks[1]);

      // Popup overlays.
      match mode {
          AppMode::ConfirmEject(index) => {
              if let Some(device) = app.devices.get(*index) {
                  let popup_area = centered_rect(60, 20, size);
                  let text = format!(
                      "Are you sure you want to eject this device?\n(Device: {})\nPress Y to confirm, N to cancel.",
                      device.name
                  );
                  let block = Block::default()
                      .borders(Borders::ALL)
                      .title("Confirm Eject")
                      .style(Style::default().fg(Color::White).bg(Color::Black));
                  let paragraph = Paragraph::new(text).block(block);
                  f.render_widget(paragraph, popup_area);
              }
          },
          AppMode::Ejected(msg) => {
              let popup_area = centered_rect(60, 20, size);
              let text = format!("{}\nPress any key to continue.", msg);
              let block = Block::default()
                  .borders(Borders::ALL)
                  .title("Ejection Result")
                  .style(Style::default().fg(Color::White).bg(Color::Black));
              let paragraph = Paragraph::new(text).block(block);
              f.render_widget(paragraph, popup_area);
          },
          _ => {}
      }
  })?;
  Ok(())
}
