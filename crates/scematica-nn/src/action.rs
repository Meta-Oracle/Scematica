use serde::{Deserialize, Serialize};

pub const ACTION_DIM: usize = 5;

/// Discrete action space for the DQN trading agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeAction {
    Hold = 0,
    BuyStandard = 1,
    BuyAggressive = 2,
    SellPartial = 3,
    SellAll = 4,
}

impl TradeAction {
    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Hold,
            1 => Self::BuyStandard,
            2 => Self::BuyAggressive,
            3 => Self::SellPartial,
            4 => Self::SellAll,
            _ => Self::Hold,
        }
    }

    pub fn index(self) -> usize { self as usize }

    pub fn is_buy(self) -> bool {
        matches!(self, Self::BuyStandard | Self::BuyAggressive)
    }

    pub fn is_sell(self) -> bool {
        matches!(self, Self::SellPartial | Self::SellAll)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hold          => "HOLD",
            Self::BuyStandard   => "BUY",
            Self::BuyAggressive => "BUY_AGG",
            Self::SellPartial   => "SELL_PARTIAL",
            Self::SellAll       => "SELL_ALL",
        }
    }
}
