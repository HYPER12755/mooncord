use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use super::{
    render,
    state::{LoginScreen, LoginState},
    terminal_events::{LoginAction, handle_terminal},
};

fn press(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn login_choices_use_shortcut_theme() {
    let state = LoginState::new(None);
    let custom = crate::tui::theme::Theme::default().with_style(
        crate::tui::theme::HighlightGroup::Shortcut,
        Style::default().fg(Color::LightMagenta),
    );

    crate::tui::theme::with_test_theme(custom, || {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| render(frame, &state))
            .expect("login choices should render");

        let buffer = terminal.backend().buffer();
        let shortcut = (0..buffer.area.height)
            .flat_map(|row| {
                (0..buffer.area.width.saturating_sub(1)).map(move |column| (column, row))
            })
            .find(|&(column, row)| {
                buffer[(column, row)].symbol() == "[" && buffer[(column + 1, row)].symbol() == "t"
            })
            .expect("token shortcut should render");
        assert_eq!(buffer[shortcut].fg, Color::LightMagenta);
    });
}

#[test]
fn token_input_rejects_invalid_header_value() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::TokenInput;
    state.token_input = "bad\ntoken".to_owned();

    let action = handle_terminal(&mut state, press(KeyCode::Enter));

    assert!(action.is_none());
    assert!(state.error.as_deref().is_some_and(|error| {
        error.contains("Token is invalid") && error.contains("valid HTTP authorization header")
    }));
}

#[test]
fn password_submit_starts_login_and_clears_password_field() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::PasswordInput;
    state.password.login = "  user@example.com  ".to_string();
    state.password.password = "password".to_string();

    let action = handle_terminal(&mut state, press(KeyCode::Enter));

    assert!(matches!(
        action,
        Some(LoginAction::StartPasswordLogin { login, password })
            if login == "user@example.com" && password == "password"
    ));
    assert!(state.password.password.is_empty());
}

#[test]
fn password_input_accepts_paste_and_renders_field_states() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::PasswordInput;
    state.password.active_field = super::state::PasswordField::Password;
    state.password.login = "테스트用户@example.com".to_owned();
    state.error = Some("old error".to_string());

    let action = handle_terminal(&mut state, paste("ab[]{};\\cd\n"));

    assert!(action.is_none());
    assert_eq!(state.password.password, "ab[]{};\\cd");
    assert!(state.error.is_none());

    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render(frame, &state))
        .expect("password form should render");

    let buffer = terminal.backend().buffer();
    for expected in [
        "Warning: Email/password login may cause a temporary account restriction.",
        "Use it with caution. Token login is recommended.",
    ] {
        let characters: Vec<_> = expected.chars().collect();
        let (start, row) = (0..buffer.area.height)
            .find_map(|row| {
                (0..=buffer.area.width.saturating_sub(characters.len() as u16)).find_map(|column| {
                    characters
                        .iter()
                        .enumerate()
                        .all(|(offset, character)| {
                            buffer[(column.saturating_add(offset as u16), row)].symbol()
                                == character.to_string()
                        })
                        .then_some((column, row))
                })
            })
            .expect("password risk warning should render");
        for column in start..start.saturating_add(characters.len() as u16) {
            assert_eq!(buffer[(column, row)].fg, Color::Red);
        }
    }

    let email_start = (0..buffer.area.height)
        .flat_map(|row| (0..buffer.area.width).map(move |column| (column, row)))
        .find(|position| buffer[*position].symbol() == "테")
        .expect("inactive CJK email should render");
    let mut column = email_start.0;
    for character in "테스트用户@example.com".chars() {
        let cell = &buffer[(column, email_start.1)];
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
        column = column.saturating_add(character.to_string().width() as u16);
    }

    let password_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, *row)].symbol())
                .collect::<String>()
                .contains("> Password")
        })
        .expect("active password field should render");
    let marker_column = (0..buffer.area.width)
        .find(|column| buffer[(*column, password_row)].symbol() == ">")
        .expect("active password marker should render");
    let active_value = "•".repeat(state.password.password.chars().count());
    let active_width = format!("> Password     {active_value}").width() as u16;
    for column in marker_column..marker_column.saturating_add(active_width) {
        assert_eq!(
            buffer[(column, password_row)].fg,
            crate::tui::theme::current().foreground(crate::tui::theme::HighlightGroup::ActiveField)
        );
    }
}

#[test]
fn token_input_accepts_bracketed_paste_text() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::TokenInput;

    handle_terminal(&mut state, paste("token-part-1\ntoken-part-2"));

    assert_eq!(state.token_input, "token-part-1token-part-2");
}
