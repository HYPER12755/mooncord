use super::super::keybindings::KeyBindings;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LoginScreen {
    ModeSelect,
    TokenInput,
    EmailPassword,
    MfaSelect,
    MfaCode,
    OauthUrl,
    Busy,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FormField {
    Email,
    // For email/password: password
    Password,
}

pub(super) struct LoginState {
    pub(super) key_bindings: KeyBindings,
    pub(super) screen: LoginScreen,
    pub(super) notice: Option<String>,
    pub(super) error: Option<String>,
    pub(super) token_input: String,
    pub(super) email: String,
    pub(super) password: String,
    pub(super) active_field: FormField,
    pub(super) in_progress: bool,
    pub(super) status: String,
    pub(super) oauth_url: Option<String>,
    pub(super) mfa_challenge_ticket: Option<String>,
    /// Whether the email/password screen is in sign-up mode (vs. sign-in).
    pub(super) signup_mode: bool,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            key_bindings: KeyBindings::default(),
            screen: LoginScreen::ModeSelect,
            notice: None,
            error: None,
            token_input: String::new(),
            email: String::new(),
            password: String::new(),
            active_field: FormField::Email,
            in_progress: false,
            status: String::new(),
            oauth_url: None,
            mfa_challenge_ticket: None,
            signup_mode: false,
        }
    }
}

impl LoginState {
    pub(super) fn reset_sensitive(&mut self) {
        self.password.clear();
        self.token_input.clear();
    }

    pub(super) fn reset_form(&mut self) {
        self.email.clear();
        self.password.clear();
        self.token_input.clear();
        self.active_field = FormField::Email;
        self.signup_mode = false;
    }
}
