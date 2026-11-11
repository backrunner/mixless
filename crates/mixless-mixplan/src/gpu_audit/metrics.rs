use serde_json::{json, Value};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Score {
    pub valid: u32,
    pub cost: f32,
}
impl Score {
    pub fn value(self) -> Option<f32> {
        (self.valid != 0).then_some(self.cost)
    }
}
#[derive(Default)]
pub(super) struct Metrics {
    pub jobs: u64,
    pub groups: u64,
    pub batches: u64,
    pub queries: u64,
    pub uncompared: u64,
    pub acceptance: u64,
    pub bits: u64,
    pub nonfinite: u64,
    pub selections: u64,
    pub comparisons: u64,
    pub max_abs: f32,
    pub max_relative: f32,
    pub max_batch: usize,
    pub gpu_seconds: f64,
    pub dispatch_seconds: f64,
    pub errors: Vec<String>,
    pub examples: Vec<Value>,
}
fn wins(score: Option<f32>, current: Option<f32>) -> bool {
    score.is_some_and(|c| c < current.unwrap_or(f32::INFINITY) - 1e-4)
}
fn winner(values: &[Option<f32>]) -> usize {
    let mut selected = 0;
    for i in 1..values.len() {
        if wins(values[i], values[selected]) {
            selected = i;
        }
    }
    selected
}
impl Metrics {
    pub fn observe(&mut self, cpu: &[Option<f32>], gpu: &[Score]) -> bool {
        let before = (
            self.acceptance,
            self.nonfinite,
            self.selections,
            self.comparisons,
        );
        assert_eq!(cpu.len(), gpu.len());
        self.groups += 1;
        self.queries += cpu.len() as u64;
        let values: Vec<_> = gpu.iter().map(|v| v.value()).collect();
        for (a, b) in cpu.iter().zip(&values) {
            if a.is_some() != b.is_some() {
                self.acceptance += 1;
            }
            if let (Some(a), Some(b)) = (a, b) {
                if a.to_bits() != b.to_bits() {
                    self.bits += 1;
                }
                if a.is_finite() && b.is_finite() {
                    self.max_abs = self.max_abs.max((a - b).abs());
                    self.max_relative = self.max_relative.max((a - b).abs() / a.abs().max(1e-30));
                } else if !(a.is_nan() && b.is_nan()) && a != b {
                    self.nonfinite += 1;
                }
            }
        }
        for i in 0..cpu.len() {
            for j in 0..cpu.len() {
                if i != j && wins(cpu[i], cpu[j]) != wins(values[i], values[j]) {
                    self.comparisons += 1;
                }
            }
        }
        if winner(cpu) != winner(&values) {
            self.selections += 1;
        }
        before
            != (
                self.acceptance,
                self.nonfinite,
                self.selections,
                self.comparisons,
            )
    }
    pub fn json(&self) -> Value {
        json!({"jobs":self.jobs,"groups":self.groups,"dispatches":self.batches,"queries":self.queries,"uncompared_queries":self.uncompared,
            "acceptance_mismatches":self.acceptance,"score_bit_differences":self.bits,"nonfinite_mismatches":self.nonfinite,
            "batch_selection_mismatches":self.selections,"strict_comparison_mismatches":self.comparisons,
            "max_absolute_difference":self.max_abs,"max_relative_difference":self.max_relative,
            "max_dispatch_queries":self.max_batch,"gpu_seconds":self.gpu_seconds,"dispatch_seconds":self.dispatch_seconds,"errors":self.errors,"decision_examples":self.examples})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_rejections_threshold_flips_and_ties_separately() {
        let cpu = [Some(1.), Some(1. - 0.0001), Some(0.5), None];
        let gpu = [
            Score { valid: 1, cost: 1. },
            Score {
                valid: 1,
                cost: 0.9998,
            },
            Score { valid: 0, cost: 0. },
            Score {
                valid: 1,
                cost: 0.1,
            },
        ];
        let mut m = Metrics::default();
        m.observe(&cpu, &gpu);
        assert_eq!(m.acceptance, 2);
        assert_eq!(m.selections, 1);
        assert!(m.comparisons > 0);
        assert_eq!(m.bits, 1);
        let same: Vec<_> = cpu
            .iter()
            .map(|v| Score {
                valid: u32::from(v.is_some()),
                cost: v.unwrap_or(0.),
            })
            .collect();
        let mut m = Metrics::default();
        m.observe(&cpu, &same);
        assert_eq!(
            (m.acceptance, m.selections, m.comparisons, m.bits),
            (0, 0, 0, 0)
        );
    }
}
