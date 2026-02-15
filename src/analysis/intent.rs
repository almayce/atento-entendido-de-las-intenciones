use serde::{Deserialize, Serialize};
use std::fmt;

/// Intent categories focused on lead identification.
/// The key question: does this person have a need we can address?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Person describes a problem they're facing
    Problem,
    /// Person asks a question, seeks information
    Question,
    /// Person explicitly requests help or assistance
    HelpRequest,
    /// Person complains about a product/service/situation
    Complaint,
    /// Person gives feedback or a suggestion
    Feedback,
    /// Neutral comment, no actionable intent
    Neutral,
    /// Person shows clear buying intent
    BuyingIntent,
    /// Spam or irrelevant content
    Spam,
}

impl Intent {
    /// Whether this intent signals a potential lead
    pub fn is_lead_signal(&self) -> bool {
        matches!(
            self,
            Intent::BuyingIntent | Intent::HelpRequest | Intent::Problem
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Intent::Problem => "Проблема",
            Intent::Question => "Вопрос",
            Intent::HelpRequest => "Запрос помощи",
            Intent::Complaint => "Жалоба",
            Intent::BuyingIntent => "Интерес к покупке",
            Intent::Feedback => "Отзыв",
            Intent::Neutral => "Нейтрально",
            Intent::Spam => "Спам",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Intent::Problem => "intent-problem",
            Intent::Question => "intent-question",
            Intent::HelpRequest => "intent-help",
            Intent::Complaint => "intent-complaint",
            Intent::BuyingIntent => "intent-buying",
            Intent::Feedback => "intent-feedback",
            Intent::Neutral => "intent-neutral",
            Intent::Spam => "intent-spam",
        }
    }

    pub fn all() -> &'static [Intent] {
        &[
            Intent::BuyingIntent,
            Intent::HelpRequest,
            Intent::Problem,
            Intent::Question,
            Intent::Complaint,
            Intent::Feedback,
            Intent::Neutral,
            Intent::Spam,
        ]
    }
}

impl fmt::Display for Intent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
