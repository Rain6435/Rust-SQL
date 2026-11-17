//! A minimal PostgreSQL wire-protocol server that asks for a cleartext password, records the
//! password it receives, and rejects it. Tests use it to prove which password a front end
//! actually sent after decrypting a saved connection.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_body(sock: &mut TcpStream) -> Vec<u8> {
    let len = sock.read_i32().await.expect("len") as usize;
    let mut body = vec![0u8; len - 4];
    sock.read_exact(&mut body).await.expect("body");
    body
}

/// Starts the server and returns its port and a handle yielding the received password.
pub async fn spawn_password_capturing_pg() -> (u16, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        let mut startup = read_body(&mut sock).await;
        if startup.starts_with(&80877103i32.to_be_bytes()) {
            sock.write_all(b"N").await.expect("ssl refuse");
            startup = read_body(&mut sock).await;
        }
        assert!(startup.starts_with(&196608i32.to_be_bytes()), "startup");
        // AuthenticationCleartextPassword
        sock.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 3])
            .await
            .expect("auth");
        assert_eq!(sock.read_u8().await.expect("tag"), b'p');
        let mut password = read_body(&mut sock).await;
        if password.last() == Some(&0) {
            password.pop();
        }
        let mut fields = Vec::new();
        for (code, value) in [
            (b'S', "FATAL"),
            (b'V', "FATAL"),
            (b'C', "28P01"),
            (b'M', "password authentication failed"),
        ] {
            fields.push(code);
            fields.extend_from_slice(value.as_bytes());
            fields.push(0);
        }
        fields.push(0);
        let mut msg = vec![b'E'];
        msg.extend_from_slice(&((fields.len() + 4) as i32).to_be_bytes());
        msg.extend_from_slice(&fields);
        let _ = sock.write_all(&msg).await;
        String::from_utf8(password).expect("utf-8 password")
    });
    (port, handle)
}
