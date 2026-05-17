//! Noise_XX transport over TCP.
//!
//! Handshake: `Noise_XX_25519_ChaChaPoly_BLAKE2s` (mutual static-key auth,
//! like WireGuard). After the 3-message XX handshake both peers know each
//! other's static public key (used for TOFU). Application messages are
//! length-delimited and chunked so payloads larger than a Noise message
//! (64 KiB) transfer transparently.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::error::{Error, Result};

const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// Max plaintext per Noise message (64 KiB − 16-byte tag), rounded down.
const CHUNK: usize = 60_000;

/// Generate a persistent X25519 static keypair `(private, public)`.
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>)> {
    let b = snow::Builder::new(NOISE_PARAMS.parse().map_err(noise)?);
    let kp = b.generate_keypair().map_err(noise)?;
    Ok((kp.private, kp.public))
}

fn noise<E: std::fmt::Display>(e: E) -> Error {
    Error::Noise(e.to_string())
}

fn write_frame(s: &mut TcpStream, data: &[u8]) -> Result<()> {
    let len: u16 = data
        .len()
        .try_into()
        .map_err(|_| Error::Protocol("noise frame too large".into()))?;
    s.write_all(&len.to_be_bytes())?;
    s.write_all(data)?;
    Ok(())
}

fn read_frame(s: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    s.read_exact(&mut len)?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// An established, encrypted session.
pub struct Transport {
    stream: TcpStream,
    noise: snow::TransportState,
    /// Peer's static public key (from the XX handshake).
    pub remote_static: Vec<u8>,
}

impl Transport {
    /// Run the XX handshake as the connecting peer.
    pub fn initiate(stream: TcpStream, static_priv: &[u8]) -> Result<Self> {
        Self::handshake(stream, static_priv, true)
    }

    /// Run the XX handshake as the listening peer.
    pub fn respond(stream: TcpStream, static_priv: &[u8]) -> Result<Self> {
        Self::handshake(stream, static_priv, false)
    }

    fn handshake(mut stream: TcpStream, static_priv: &[u8], initiator: bool) -> Result<Self> {
        let builder =
            snow::Builder::new(NOISE_PARAMS.parse().map_err(noise)?).local_private_key(static_priv);
        let mut hs = if initiator {
            builder.build_initiator().map_err(noise)?
        } else {
            builder.build_responder().map_err(noise)?
        };

        let mut buf = vec![0u8; 65535];
        let mut my_turn = initiator;
        while !hs.is_handshake_finished() {
            if my_turn {
                let n = hs.write_message(&[], &mut buf).map_err(noise)?;
                write_frame(&mut stream, &buf[..n])?;
            } else {
                let msg = read_frame(&mut stream)?;
                hs.read_message(&msg, &mut buf).map_err(noise)?;
            }
            my_turn = !my_turn;
        }
        let remote_static = hs
            .get_remote_static()
            .map(|s| s.to_vec())
            .ok_or_else(|| Error::Protocol("peer did not present a static key".into()))?;
        let noise = hs.into_transport_mode().map_err(noise)?;
        Ok(Self {
            stream,
            noise,
            remote_static,
        })
    }

    /// Send one logical message (chunked + Noise-encrypted).
    pub fn send(&mut self, plaintext: &[u8]) -> Result<()> {
        let mut out = vec![0u8; 65535];
        for chunk in plaintext.chunks(CHUNK).chain(if plaintext.is_empty() {
            // still send a single empty chunk so recv() terminates
            Some([].as_slice())
        } else {
            None
        }) {
            let n = self.noise.write_message(chunk, &mut out).map_err(noise)?;
            write_frame(&mut self.stream, &out[..n])?;
        }
        // Zero-length terminator frame marks end of logical message.
        write_frame(&mut self.stream, &[])?;
        Ok(())
    }

    /// Receive one logical message.
    pub fn recv(&mut self) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 65535];
        let mut msg = Vec::new();
        loop {
            let frame = read_frame(&mut self.stream)?;
            if frame.is_empty() {
                break;
            }
            let n = self.noise.read_message(&frame, &mut out).map_err(noise)?;
            msg.extend_from_slice(&out[..n]);
        }
        Ok(msg)
    }
}
