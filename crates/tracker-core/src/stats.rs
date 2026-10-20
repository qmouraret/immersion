//! Statistiques glissantes, pour mesurer la gigue (bruit à l'arrêt).

use std::collections::VecDeque;

/// Moyenne et écart-type sur les `capacity` dernières valeurs.
#[derive(Debug, Clone)]
pub struct Rolling {
    capacity: usize,
    values: VecDeque<f64>,
}

impl Rolling {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            values: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(v);
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn mean(&self) -> Option<f64> {
        (!self.values.is_empty())
            .then(|| self.values.iter().sum::<f64>() / self.values.len() as f64)
    }

    /// Écart-type (population). `None` tant qu'il y a moins de 2 valeurs.
    pub fn std_dev(&self) -> Option<f64> {
        if self.values.len() < 2 {
            return None;
        }
        let mean = self.mean()?;
        let var =
            self.values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / self.values.len() as f64;
        Some(var.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecart_type_d_une_alternance() {
        let mut r = Rolling::new(4);
        for v in [1.0, -1.0, 1.0, -1.0] {
            r.push(v);
        }
        assert_eq!(r.mean(), Some(0.0));
        assert_eq!(r.std_dev(), Some(1.0));
    }

    #[test]
    fn fenetre_glissante() {
        let mut r = Rolling::new(2);
        for v in [100.0, 3.0, 5.0] {
            r.push(v);
        }
        assert_eq!(r.mean(), Some(4.0)); // 100 est sorti de la fenêtre
    }
}
