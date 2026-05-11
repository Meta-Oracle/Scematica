use proptest::prelude::*;
use crate::chat::ChatPanel;
use ratatui::event::KeyEvent;
use crossterm::event::KeyCode;

proptest! {
    #[test]
    fn test_handle_key_input_buffer(
        keys: Vec<char>
    ) {
        let mut panel = ChatPanel::new();
        let mut expected_buffer = String::new();

        for key_char in keys {
            let key_event = KeyEvent::new(KeyCode::Char(key_char), ratatui::input::KeyModifiers::NONE);
            panel.handle_key(key_event);
            expected_buffer.push(key_char);
        }

        assert_eq!(panel.input_buffer, expected_buffer);
    }

    #[test]
    fn test_handle_key_backspace(
        initial_text: String,
        num_backspaces: u32
    ) {
        let mut panel = ChatPanel::new();
        panel.input_buffer = initial_text.clone();
        
        let mut expected_buffer = initial_text;

        for _ in 0..num_backspaces {
            let key_event = KeyEvent::new(KeyCode::Backspace, ratatui::input::KeyModifiers::NONE);
            panel.handle_key(key_event);
            if !expected_buffer.is_empty() {
                expected_buffer.pop();
            }
        }

        assert_eq!(panel.input_buffer, expected_buffer);
    }
}
