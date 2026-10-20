use std::time::Instant;

/// Une image capturée, en RGB 8 bits entrelacé (R, G, B, R, G, B, ...).
///
/// `Frame` possède ses pixels (`Vec<u8>`) : quand on l'envoie d'un thread à
/// l'autre, c'est la propriété du buffer qui voyage, sans copie.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Pixels, ligne par ligne, 3 octets (R, G, B) par pixel.
    pub rgb: Vec<u8>,
    /// Instant de capture, pour mesurer la latence du pipeline.
    pub captured_at: Instant,
    /// Numéro de séquence croissant, pour détecter les images perdues.
    pub seq: u64,
}

impl Frame {
    /// Construit une image en vérifiant la cohérence taille / buffer.
    ///
    /// Retourne `None` si le buffer n'a pas la taille `width * height * 3`.
    /// L'horodatage est fourni par l'appelant : la caméra (à la réception) ou,
    /// plus tard, un enregistrement rejoué (avec ses instants d'origine).
    pub fn new(
        width: u32,
        height: u32,
        rgb: Vec<u8>,
        seq: u64,
        captured_at: Instant,
    ) -> Option<Self> {
        let expected = width as usize * height as usize * 3;
        (rgb.len() == expected).then_some(Self {
            width,
            height,
            rgb,
            captured_at,
            seq,
        })
    }

    /// Âge de l'image depuis sa capture, en millisecondes.
    pub fn age_ms(&self) -> f64 {
        self.captured_at.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepte_un_buffer_de_bonne_taille() {
        let f = Frame::new(4, 2, vec![0; 4 * 2 * 3], 7, Instant::now()).unwrap();
        assert_eq!((f.width, f.height, f.seq), (4, 2, 7));
    }

    #[test]
    fn refuse_un_buffer_incoherent() {
        assert!(Frame::new(4, 2, vec![0; 10], 0, Instant::now()).is_none());
    }
}
