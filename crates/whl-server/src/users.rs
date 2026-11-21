use whl_core::tier::Tiers;
use whl_store::Store;
use whl_store::Uuid;
use whl_store::quota::{QuotaStoreError, SetTierOutcome, UserSelector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierChange {
    pub user: Uuid,
    pub previous: String,
    pub tier: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SetTierError {
    #[error("{0:?} is not a tier of this instance, or cannot be assigned to a user")]
    UnknownTier(String),
    #[error("no user matches {0:?}")]
    NoSuchUser(String),
    #[error("{count} users have that email address, name one of them by id: {ids}")]
    Ambiguous { count: usize, ids: String },
    #[error("the database failed: {0}")]
    Store(#[from] QuotaStoreError),
}

pub fn user_selector(argument: &str) -> UserSelector {
    match Uuid::try_parse(argument.trim()) {
        Ok(id) => UserSelector::Id(id),
        Err(_) => UserSelector::Email(argument.trim().to_owned()),
    }
}

pub async fn set_user_tier(
    store: &Store,
    tiers: &Tiers,
    user: &str,
    tier: &str,
) -> Result<TierChange, SetTierError> {
    if !tiers.is_assignable(tier) {
        return Err(SetTierError::UnknownTier(tier.to_owned()));
    }
    match store.set_user_tier(&user_selector(user), tier).await? {
        SetTierOutcome::Changed { user, previous } => Ok(TierChange {
            user,
            previous,
            tier: tier.to_owned(),
        }),
        SetTierOutcome::NotFound => Err(SetTierError::NoSuchUser(user.to_owned())),
        SetTierOutcome::Ambiguous { users } => Err(SetTierError::Ambiguous {
            count: users.len(),
            ids: users
                .iter()
                .map(|id| id.hyphenated().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}
