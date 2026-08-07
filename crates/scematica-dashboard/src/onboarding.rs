#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingStep {
    Welcome,
    VerifyWallet,
    StrategyTuning,
    DemoSimulation,
    Completed,
}

#[derive(Debug)]
pub struct OnboardingManager {
    pub current_step: OnboardingStep,
}

impl OnboardingManager {
    pub fn new() -> Self {
        Self {
            current_step: OnboardingStep::Welcome,
        }
    }

    pub fn next(&mut self) {
        self.current_step = match self.current_step {
            OnboardingStep::Welcome => OnboardingStep::VerifyWallet,
            OnboardingStep::VerifyWallet => OnboardingStep::StrategyTuning,
            OnboardingStep::StrategyTuning => OnboardingStep::DemoSimulation,
            OnboardingStep::DemoSimulation => OnboardingStep::Completed,
            OnboardingStep::Completed => OnboardingStep::Completed,
        };
    }
}
