//! Récepteur de diagnostic : affiche les paquets OpenTrack reçus.
//!
//! `cargo run -p output --example receive -- [port]` (4242 par défaut), puis
//! activer la sortie dans Immersion. Ne pas lancer en même temps qu'OpenTrack
//! sur la même machine : un seul programme peut écouter un port donné.
use std::net::UdpSocket;
use std::time::Instant;

fn main() -> std::io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(4242);
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    println!("Écoute sur le port UDP {port}… (Ctrl+C pour arrêter)");
    // Le chronomètre démarre au premier paquet, pas au lancement du programme.
    let (mut buf, mut count, mut since) = ([0u8; 128], 0u32, None::<Instant>);
    loop {
        let (n, from) = socket.recv_from(&mut buf)?;
        count += 1;
        let started = *since.get_or_insert_with(Instant::now);
        let Some(p) = output::decode(&buf[..n]) else {
            println!("paquet de {n} octets ignoré (48 attendus) depuis {from}");
            continue;
        };
        // Une ligne par seconde suffit à lire ; on compte le débit.
        if started.elapsed().as_secs_f64() >= 1.0 {
            println!(
                "{count:3} paquets/s depuis {from} | lacet {:+6.1}° tangage {:+6.1}° roulis {:+6.1}° | x {:+5.1} y {:+5.1} z {:+5.1} cm",
                p.angles.yaw, p.angles.pitch, p.angles.roll, p.x, p.y, p.z
            );
            (count, since) = (0, Some(Instant::now()));
        }
    }
}
