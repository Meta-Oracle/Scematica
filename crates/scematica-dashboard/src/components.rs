use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

// UI Theme Constants
pub const COLOR_BG: Color = Color::Rgb(10, 10, 10); // Deep Black
pub const COLOR_ACCENT: Color = Color::Rgb(211, 47, 47); // Crimson Red
pub const COLOR_TEXT: Color = Color::White; // White

pub struct LoaderSpinner {
    pub frame: usize,
}

impl LoaderSpinner {
    pub fn new() -> Self {
        Self { frame: 0 }
    }

    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % 4;
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let symbol = match self.frame {
            0 => "/",
            1 => "-",
            2 => "\\",
            _ => "|",
        };

        let block = Block::default()
            .title(" AI Processing ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_ACCENT));

        let p = Paragraph::new(format!("  {}  ", symbol))
            .style(Style::default().fg(COLOR_ACCENT))
            .alignment(Alignment::Center)
            .block(block);

        f.render_widget(p, area);
    }
}
