//! Panneau « Sortie » : envoi de la pose vers OpenTrack.

use std::collections::VecDeque;

use eframe::egui::{self, Color32};
use output::OutputSettings;
use tracker_core::processing::AXES;

use crate::pipeline::SinkStatus;

pub struct OutputPanel {
    /// Réglages appliqués (et enregistrés dans le profil).
    pub settings: OutputSettings,
    /// Adresse en cours de saisie : appliquée seulement sur « Appliquer » ou Entrée,
    /// pour ne pas reconnecter à chaque touche.
    host_edit: String,
    port_edit: u16,
    /// Instants des paquets envoyés pendant la dernière seconde.
    sent_at: VecDeque<f64>,
    last: SinkStatus,
}

impl OutputPanel {
    pub fn new(settings: OutputSettings) -> Self {
        Self {
            host_edit: settings.host.clone(),
            port_edit: settings.port,
            settings,
            sent_at: VecDeque::new(),
            last: SinkStatus::default(),
        }
    }

    /// Remplace les réglages (chargement d'un profil).
    pub fn set(&mut self, settings: OutputSettings) {
        *self = Self::new(settings);
    }

    /// État de l'envoi pour une image (instant en secondes).
    pub fn on_frame(&mut self, t: f64, status: SinkStatus) {
        if status.sent {
            self.sent_at.push_back(t);
        }
        while self.sent_at.front().is_some_and(|s| t - s > 1.0) {
            self.sent_at.pop_front();
        }
        self.last = status;
    }

    /// Dessine le panneau. Renvoie de nouveaux réglages s'ils ont changé.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<OutputSettings> {
        let mut changed = false;
        egui::CollapsingHeader::new("Sortie vers OpenTrack (UDP)")
            .default_open(true)
            .show(ui, |ui| {
                changed |= ui
                    .checkbox(&mut self.settings.enabled, "Envoyer la pose")
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Adresse");
                    let host = ui
                        .add(egui::TextEdit::singleline(&mut self.host_edit).desired_width(110.0));
                    ui.label(":");
                    ui.add(egui::DragValue::new(&mut self.port_edit).range(1..=65535));
                    let edited = self.host_edit != self.settings.host
                        || self.port_edit != self.settings.port;
                    let enter = host.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(edited, egui::Button::new("Appliquer"))
                        .clicked()
                        || (edited && enter)
                    {
                        self.settings.host = self.host_edit.trim().to_owned();
                        self.settings.port = self.port_edit;
                        changed = true;
                    }
                });
                ui.small(
                    "OpenTrack : entrée « UDP over network », port 4242. Préférer une adresse IP.",
                );

                ui.horizontal_wrapped(|ui| {
                    ui.label("Inverser :");
                    for (i, name) in AXES.iter().enumerate() {
                        changed |= ui.checkbox(&mut self.settings.invert[i], *name).changed();
                    }
                });

                let s = &self.last;
                if !self.settings.enabled {
                    ui.label("Envoi désactivé.");
                } else if s.connecting {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Connexion…");
                    });
                } else if let Some(target) = &s.target {
                    ui.label(format!("{target} : {} paquets/s", self.sent_at.len()));
                    if self.sent_at.is_empty() {
                        ui.small("Aucun paquet : pas de visage détecté ?");
                    }
                }
                if let Some(e) = &s.error {
                    ui.colored_label(Color32::from_rgb(240, 90, 90), e);
                }
            });
        changed.then(|| self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debit_sur_la_derniere_seconde() {
        let mut p = OutputPanel::new(OutputSettings::default());
        for i in 0..90 {
            p.on_frame(
                f64::from(i) / 30.0,
                SinkStatus {
                    sent: true,
                    ..Default::default()
                },
            );
        }
        // Fenêtre d'une seconde : les 31 derniers instants (bornes incluses).
        assert!((30..=31).contains(&p.sent_at.len()), "{}", p.sent_at.len());
        p.on_frame(10.0, SinkStatus::default());
        assert!(p.sent_at.is_empty(), "plus rien d'envoyé depuis 7 s");
    }
}
