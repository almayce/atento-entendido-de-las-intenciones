use serde::{Deserialize, Serialize};
use std::fmt;

/// Intent categories for B2B lead identification (smart Telegram monitoring service).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Business owner, co-founder, CEO, entrepreneur
    BusinessOwner,
    /// Marketer, sales manager, lead gen specialist
    Marketer,
    /// Realtor or real estate agency representative
    RealtorAgency,
    /// Property investor managing a portfolio at scale
    Investor,
    /// IT/tech business — potential partner or referral
    ItBusiness,
    /// Person expresses a pain point Telegram monitoring solves
    PainSignal,
    /// Regular individual (not a business lead)
    Individual,
    /// Neutral comment, no business context
    Neutral,
    /// Spam or irrelevant content
    Spam,
}

impl Intent {
    pub fn label(&self) -> &'static str {
        match self {
            Intent::BusinessOwner => "Владелец бизнеса",
            Intent::Marketer => "Маркетолог / продажи",
            Intent::RealtorAgency => "Риэлтор / агентство",
            Intent::Investor => "Инвестор",
            Intent::ItBusiness => "IT / технологии",
            Intent::PainSignal => "Боль бизнеса",
            Intent::Individual => "Физлицо",
            Intent::Neutral => "Нейтрально",
            Intent::Spam => "Спам",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Intent::BusinessOwner => "intent-buying",
            Intent::Marketer => "intent-help",
            Intent::RealtorAgency => "intent-question",
            Intent::Investor => "intent-feedback",
            Intent::ItBusiness => "intent-help",
            Intent::PainSignal => "intent-problem",
            Intent::Individual => "intent-neutral",
            Intent::Neutral => "intent-neutral",
            Intent::Spam => "intent-spam",
        }
    }

    pub fn all() -> &'static [Intent] {
        &[
            Intent::BusinessOwner,
            Intent::Marketer,
            Intent::RealtorAgency,
            Intent::Investor,
            Intent::ItBusiness,
            Intent::PainSignal,
            Intent::Individual,
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
