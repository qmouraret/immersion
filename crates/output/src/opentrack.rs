//! Protocole « UDP over network » d'OpenTrack.
//!
//! D'après le code source d'OpenTrack (`tracker-udp/ftnoir_tracker_udp.cpp`,
//! `api/plugin-api.hpp`) : chaque datagramme contient **6 `f64` au format natif**
//! (48 octets), dans l'ordre `[TX, TY, TZ, lacet, tangage, roulis]` ; translations
//! en cm, rotations en degrés ; port 4242 par défaut. OpenTrack ignore un paquet
//! contenant un NaN ou un infini.
//!
//! « Natif » : x86-64 et ARM64 sont petit-boutistes (little-endian), on écrit
//! donc explicitement en petit-boutiste, ce qui reste juste entre un Mac et un PC.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use tracker_core::Pose6;

use crate::{OutputError, PoseSink};

/// Taille d'un paquet : 6 × 8 octets.
pub const PACKET_LEN: usize = 48;

/// Convertit une pose (convention interne) en paquet OpenTrack.
///
/// `invert` suit l'ordre interne [lacet, tangage, roulis, x, y, z]. Une valeur
/// non finie est remplacée par 0, pour qu'OpenTrack ne rejette pas le paquet.
pub fn encode(pose: &Pose6, invert: &[bool; 6]) -> [u8; PACKET_LEN] {
    let ours = pose.to_array();
    let v = |i: usize| {
        let x = if invert[i] { -ours[i] } else { ours[i] };
        if x.is_finite() { x } else { 0.0 }
    };
    // Ordre OpenTrack : translations d'abord, puis rotations.
    let values = [v(3), v(4), v(5), v(0), v(1), v(2)];
    let mut out = [0u8; PACKET_LEN];
    for (chunk, value) in out.as_chunks_mut::<8>().0.iter_mut().zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// Opération inverse de [`encode`] (sans inversion d'axes) : pour les tests et
/// le récepteur de diagnostic.
pub fn decode(packet: &[u8]) -> Option<Pose6> {
    let bytes: &[u8; PACKET_LEN] = packet.get(..PACKET_LEN)?.try_into().ok()?;
    let mut values = [0.0; 6];
    for (v, chunk) in values.iter_mut().zip(bytes.as_chunks::<8>().0) {
        *v = f64::from_le_bytes(*chunk);
    }
    let [x, y, z, yaw, pitch, roll] = values;
    Some(Pose6::from_array([yaw, pitch, roll, x, y, z]))
}

/// Envoi UDP vers OpenTrack.
pub struct OpenTrackUdp {
    socket: UdpSocket,
    target: SocketAddr,
    invert: [bool; 6],
}

impl OpenTrackUdp {
    /// Prépare l'envoi vers `host:port` (nom ou adresse IP).
    pub fn new(host: &str, port: u16, invert: [bool; 6]) -> Result<Self, OutputError> {
        let target = (host, port)
            .to_socket_addrs()
            .map_err(|e| OutputError::Address(format!("{host}:{port}"), e.to_string()))?
            .next()
            .ok_or_else(|| {
                OutputError::Address(format!("{host}:{port}"), "aucune adresse".into())
            })?;
        // Port local choisi par l'OS ; même famille (IPv4 / IPv6) que la cible.
        let local = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(local)?;
        // Envoi non bloquant : le pipeline ne doit jamais attendre le réseau.
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            target,
            invert,
        })
    }
}

impl PoseSink for OpenTrackUdp {
    fn send(&mut self, pose: &Pose6) -> Result<(), OutputError> {
        match self
            .socket
            .send_to(&encode(pose, &self.invert), self.target)
        {
            Ok(_) => Ok(()),
            // Tampon d'envoi plein : on perd ce paquet, le suivant arrive dans 33 ms.
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn describe(&self) -> String {
        format!("OpenTrack UDP → {}", self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tracker_core::pose::Angles;

    fn pose() -> Pose6 {
        Pose6 {
            angles: Angles {
                yaw: 12.5,
                pitch: -3.0,
                roll: 1.5,
            },
            x: 2.0,
            y: -1.0,
            z: 4.0,
        }
    }

    #[test]
    fn ordre_opentrack_translations_puis_rotations() {
        let packet = encode(&pose(), &[false; 6]);
        let first = f64::from_le_bytes(packet[0..8].try_into().unwrap());
        let fourth = f64::from_le_bytes(packet[24..32].try_into().unwrap());
        assert_eq!(first, 2.0, "TX = x");
        assert_eq!(fourth, 12.5, "4e valeur = lacet");
    }

    #[test]
    fn aller_retour() {
        assert_eq!(decode(&encode(&pose(), &[false; 6])), Some(pose()));
    }

    #[test]
    fn inversion_par_axe() {
        let mut invert = [false; 6];
        invert[0] = true; // lacet
        invert[5] = true; // z
        let back = decode(&encode(&pose(), &invert)).unwrap();
        assert_eq!(back.angles.yaw, -12.5);
        assert_eq!(back.z, -4.0);
        assert_eq!(back.angles.pitch, -3.0, "les autres axes ne bougent pas");
    }

    #[test]
    fn jamais_de_nan_dans_un_paquet() {
        let bad = Pose6 {
            x: f64::NAN,
            z: f64::INFINITY,
            ..pose()
        };
        let back = decode(&encode(&bad, &[false; 6])).unwrap();
        assert_eq!((back.x, back.z), (0.0, 0.0));
    }

    #[test]
    fn paquet_trop_court() {
        assert!(decode(&[0u8; 40]).is_none());
    }

    #[test]
    fn envoi_reel_en_local() {
        // Récepteur sur un port libre de la boucle locale, comme OpenTrack.
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();

        let mut sink = OpenTrackUdp::new("127.0.0.1", port, [false; 6]).unwrap();
        sink.send(&pose()).unwrap();

        let mut buf = [0u8; 128];
        let (n, _) = receiver.recv_from(&mut buf).unwrap();
        assert_eq!(n, PACKET_LEN);
        assert_eq!(decode(&buf[..n]), Some(pose()));
        assert!(sink.describe().contains(&port.to_string()));
    }

    #[test]
    fn adresse_invalide() {
        // Adresse vide : rejetée sans requête DNS (un nom invalide peut prendre 5 s).
        assert!(matches!(
            OpenTrackUdp::new("", 4242, [false; 6]),
            Err(OutputError::Address(..))
        ));
    }
}
