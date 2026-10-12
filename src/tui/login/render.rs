use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::theme;

fn render_app_header(frame: &mut Frame) {
    let area = frame.area();
    if area.height == 0 {
        return;
    }
    let header = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            theme::current().style(theme::HighlightGroup::HeaderTitle),
        )))
        .alignment(Alignment::Left),
        header,
    );
}

use super::state::{LoginScreen, FormField};
use super::LoginState;

pub(super) fn render(frame: &mut Frame, state: &LoginState) {
    clear_area(frame, frame.area());
    match state.screen {
        LoginScreen::ModeSelect => render_mode_select(frame, state),
        LoginScreen::TokenInput => render_token_input(frame, state),
        LoginScreen::EmailPassword => render_email_password(frame, state),
        LoginScreen::MfaSelect | LoginScreen::MfaCode => render_mfa_fallback(frame, state),
        LoginScreen::OauthUrl => render_oauth_url(frame, state),
        LoginScreen::Busy => render_busy(frame, state),
    }
    render_app_header(frame);
}

fn render_mode_select(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 20, frame.area());
    let key_bindings = &state.key_bindings;

    let mut lines = vec![
        Line::from(Span::styled("Login", accent_style())),
        Line::from(""),
        Line::from("Choose how you want to log in:"),
        Line::from(""),
        choice_line(
            key_bindings.login_token_choice_prefix(),
            "Use existing Supabase JWT token (paste a token)",
        ),
        choice_line(
            key_bindings.login_password_choice_prefix(),
            "Email / password",
        ),
        choice_line(
            key_bindings.login_github_choice_prefix(),
            "GitHub OAuth (opens browser)",
        ),
        choice_line(
            key_bindings.login_google_choice_prefix(),
            "Google OAuth (opens browser)",
        ),
        Line::from(""),
    ];

    if let Some(notice) = &state.notice {
        lines.push(notice_line(notice));
        lines.push(Line::from(""));
    }
    if let Some(error) = &state.error {
        lines.push(error_line(error));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        key_bindings.login_cancel_quit_label(),
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " Login ", lines);
}

fn render_token_input(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 14, frame.area());
    let masked = mask_chars(&state.token_input);
    let key_bindings = &state.key_bindings;

    let persistence_text = if state.notice.is_some() {
        "Paste your token below. It will be used for this session.".to_owned()
    } else {
        format!(
            "Paste your token below. It will be saved to {}.",
            crate::token_store::credential_path_display()
        )
    };

    let mut lines = vec![
        Line::from(Span::styled("Token login", accent_style())),
        Line::from(""),
        Line::from(persistence_text),
        Line::from(""),
        Line::from(vec![
            Span::styled("> Token  ", active_style()),
            Span::styled(masked, active_style()),
        ]),
    ];

    if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(""));
    }

    if let Some(notice) = &state.notice {
        lines.push(notice_line(notice));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        key_bindings.login_token_input_label(),
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " Token ", lines);
}

fn render_email_password(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(82, 16, frame.area());
    let key_bindings = &state.key_bindings;
    let password_mask = mask_chars(&state.password);
    let email_active = state.active_field == FormField::Email;
    let password_active = state.active_field == FormField::Password;
    let email_style = if email_active { active_style() } else { dim_style() };
    let password_style = if password_active { active_style() } else { dim_style() };
    let email_marker = if email_active { "> " } else { "  " };
    let password_marker = if password_active { "> " } else { "  " };

    let mode_label = if state.signup_mode {
        "Email / password signup"
    } else {
        "Email / password login"
    };

    let mut lines = vec![
        Line::from(Span::styled(mode_label, accent_style())),
        Line::from(""),
        Line::from("Use your Supabase Auth credentials."),
        Line::from(""),
    ];

    if state.in_progress {
        lines.push(notice_line(&state.status));
    } else if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(format!("{email_marker}Email    "), email_style),
        Span::styled(state.email.clone(), email_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("{password_marker}Password "), password_style),
        Span::styled(password_mask, password_style),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        key_bindings.login_password_input_label(),
        dim_style(),
    )));

    if state.signup_mode {
        lines.push(Line::from(Span::styled(
            "[Shift+S] Back to login (toggle signup mode)",
            dim_style(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "[Shift+S] Toggle signup mode (create new account)",
            dim_style(),
        )));
    }

    render_wrapped_login_panel(frame, area, " Email Login ", lines);
}

fn render_busy(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 10, frame.area());
    let key_bindings = &state.key_bindings;

    let lines = vec![
        Line::from(Span::styled("Login", accent_style())),
        Line::from(""),
        Line::from(Span::raw(state.status.clone())),
        Line::from(""),
        Line::from(Span::styled(
            key_bindings.login_cancel_quit_label(),
            dim_style(),
        )),
    ];

    render_wrapped_login_panel(frame, area, " Login ", lines);
}

fn render_oauth_url(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 14, frame.area());
    let key_bindings = &state.key_bindings;

    let lines = vec![
        Line::from(Span::styled("OAuth login", accent_style())),
        Line::from(""),
        Line::from("Open this URL in your browser to authorize:"),
        Line::from(""),
        Line::from(Span::raw(state.oauth_url.clone().unwrap_or_default())),
        Line::from(""),
        Line::from("Press Enter once you've completed authorization, or Esc to cancel."),
        Line::from(""),
        Line::from(Span::styled(
            key_bindings.login_back_quit_label(),
            dim_style(),
        )),
    ];

    render_wrapped_login_panel(frame, area, " OAuth ", lines);
}

fn render_mfa_fallback(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 10, frame.area());
    let mut lines = vec![
        Line::from(Span::styled("Login", accent_style())),
        Line::from(""),
        Line::from("Multi-factor authentication is not supported in this login flow."),
        Line::from(""),
    ];
    if let Some(error) = &state.error {
        lines.push(error_line(error));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc back | Ctrl-C quit",
        dim_style(),
    )));
    render_wrapped_login_panel(frame, area, " Login ", lines);
}

fn render_wrapped_login_panel(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
) {
    clear_area(frame, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(login_block(title)),
        area,
    );
}

fn accent_style() -> Style {
    theme::current().style(theme::HighlightGroup::LoginTitle)
}

fn dim_style() -> Style {
    theme::current().style(theme::HighlightGroup::LoginHint)
}

fn active_style() -> Style {
    theme::current().style(theme::HighlightGroup::Strong)
}

fn error_line(value: impl AsRef<str>) -> Line<'static> {
    Line::from(Span::styled(
        value.as_ref().to_owned(),
        theme::current().style(theme::HighlightGroup::Error),
    ))
}

fn notice_line(value: impl AsRef<str>) -> Line<'static> {
    Line::from(Span::styled(
        value.as_ref().to_owned(),
        theme::current().style(theme::HighlightGroup::Strong),
    ))
}

fn choice_line(key: &'static str, text: &'static str) -> Line<'static> {
    Line::from(vec![Span::styled(key, key_style()), Span::raw(text)])
}

fn key_style() -> Style {
    theme::current().style(theme::HighlightGroup::Shortcut)
}

fn login_block(title: &'static str) -> Block<'static> {
    let theme = theme::current();
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(theme.border_type(theme::BorderSurface::Login))
        .border_style(theme.style(theme::HighlightGroup::LoginBorder))
        .title_style(theme.style(theme::HighlightGroup::LoginTitle))
}

fn clear_area(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(theme::current().style(theme::HighlightGroup::Normal)),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);

    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    horizontal
}

fn mask_chars(value: &str) -> String {
    "•".repeat(value.chars().count())
}
