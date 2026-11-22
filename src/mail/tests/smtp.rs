use super::*;
use lettre::transport::smtp::client::Certificate;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, crypto::ring, pki_types::PrivatePkcs8KeyDer},
};

async fn line<S: AsyncRead + Unpin>(socket: &mut BufReader<S>) -> String {
    let mut line = String::new();
    socket.read_line(&mut line).await.unwrap();
    line
}

async fn reply<S: AsyncRead + AsyncWrite + Unpin>(socket: &mut BufReader<S>, value: &[u8]) {
    socket.get_mut().write_all(value).await.unwrap();
    socket.get_mut().flush().await.unwrap();
}

/// A synthetic loopback mail sink. It performs a real TLS handshake, accepts
/// only the reserved .invalid recipient, and never relays anything.
async fn server(
    starttls: bool,
    accept_messages: Vec<bool>,
) -> Result<(u16, Certificate, JoinHandle<Vec<String>>)> {
    server_during_data(starttls, accept_messages, || {}).await
}

async fn server_during_data(
    starttls: bool,
    accept_messages: Vec<bool>,
    mut during_data: impl FnMut() + Send + 'static,
) -> Result<(u16, Certificate, JoinHandle<Vec<String>>)> {
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])?;
    let cert = key.cert.der().clone();
    let root = Certificate::from_der(cert.to_vec())?;
    let tls = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![cert],
            PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
        )?;
    let acceptor = TlsAcceptor::from(Arc::new(tls));
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let handle = tokio::spawn(async move {
        let mut contents = Vec::new();
        for accepted in accept_messages {
            let (socket, addr) = listener.accept().await.unwrap();
            assert!(addr.ip().is_loopback());
            let socket = if starttls {
                let mut io = BufReader::new(socket);
                reply(&mut io, b"220 local test\r\n").await;
                assert!(line(&mut io).await.starts_with("EHLO "));
                reply(&mut io, b"250-local\r\n250 STARTTLS\r\n").await;
                assert_eq!(line(&mut io).await, "STARTTLS\r\n");
                reply(&mut io, b"220 Ready for TLS\r\n").await;
                io.into_inner()
            } else {
                socket
            };
            let mut io = BufReader::new(acceptor.accept(socket).await.unwrap());
            if !starttls {
                reply(&mut io, b"220 local test\r\n").await;
            }
            assert!(line(&mut io).await.starts_with("EHLO "));
            reply(&mut io, b"250-local\r\n250 8BITMIME\r\n").await;
            assert!(
                line(&mut io)
                    .await
                    .starts_with("MAIL FROM:<alerts@example.invalid>")
            );
            reply(&mut io, b"250 Sender OK\r\n").await;
            assert_eq!(line(&mut io).await, "RCPT TO:<hunter@example.invalid>\r\n");
            reply(&mut io, b"250 Recipient OK\r\n").await;
            assert_eq!(line(&mut io).await, "DATA\r\n");
            reply(&mut io, b"354 Start mail\r\n").await;
            let mut message = String::new();
            loop {
                let value = line(&mut io).await;
                assert!(!value.is_empty(), "DATA interrupted");
                if value == ".\r\n" {
                    break;
                }
                message.push_str(&value);
            }
            contents.push(message);
            during_data();
            reply(
                &mut io,
                if accepted {
                    b"250 Queued\r\n"
                } else {
                    b"451 Try later\r\n"
                },
            )
            .await;
            if line(&mut io).await == "QUIT\r\n" {
                reply(&mut io, b"221 Bye\r\n").await;
            }
        }
        contents
    });
    Ok((port, root, handle))
}

#[tokio::test]
async fn digest_retries_with_stable_message_id_then_survives_restart_without_a_second_send()
-> Result<()> {
    let (port, ca, server) = server(false, vec![false, true]).await?;
    let tmp = TempDir::new()?;
    let mut service = MailService::new(tmp.path().into());
    service.test_ca = Some(ca.clone());
    let app = service.router();
    assert_eq!(
        request(&app, "POST", "/api/mail", configuration(port, "tls"))
            .await
            .0,
        StatusCode::OK
    );
    let path = tmp.path().join("test.sqlite3");
    let db = Connection::open(&path)?;
    migrate(&db)?;
    let mode = digest("UTC", "10:00");
    let observed = Utc::now()
        .date_naive()
        .and_hms_opt(8, 0, 0)
        .unwrap()
        .and_utc()
        + Duration::days(3);
    let app = service.clone().with_database(Some(path.clone())).router();
    for id in ["1", "2", "1"] {
        enqueue(
            &db,
            "a",
            "Règle A",
            "Bloodborne",
            &stream(id),
            &mode,
            observed,
        )?;
    }
    let rules = vec!["a".into()];
    assert!(!service.dispatch_next(&path, observed, &rules).await?);
    let (_, scheduled) = request(&app, "GET", "/api/mail/delivery", Value::Null).await;
    assert_eq!(scheduled["available"], true);
    assert_eq!(scheduled["scheduled_count"], 1);
    assert_eq!(scheduled["failed_count"], 0);
    let overdue = observed + Duration::days(1);
    assert!(
        !service
            .dispatch_next(&path, overdue, &["other".into()])
            .await?
    );
    assert!(service.dispatch_next(&path, overdue, &rules).await?);
    assert!(!service.dispatch_next(&path, overdue, &rules).await?);
    let (_, failed) = request(&app, "GET", "/api/mail/delivery", Value::Null).await;
    assert_eq!(failed["pending_count"], 1);
    assert_eq!(failed["failed_count"], 1);
    assert_eq!(failed["error_code"], "temporary");
    assert_eq!(failed["scheduled_count"], 0);
    assert_eq!(failed["rule_name"], "Règle A");
    assert!(
        failed["next_attempt_at"]
            .as_i64()
            .is_some_and(|retry| retry > overdue.timestamp())
    );
    assert!(!failed.to_string().contains("example.invalid"));
    let mut restarted = MailService::new(tmp.path().into());
    restarted.test_ca = Some(ca);
    let later = overdue + Duration::minutes(1);
    assert!(restarted.dispatch_next(&path, later, &rules).await?);
    assert!(
        !restarted
            .dispatch_next(&path, later + Duration::days(3), &rules)
            .await?
    );
    let (_, cleared) = request(&app, "GET", "/api/mail/delivery", Value::Null).await;
    assert_eq!(cleared["pending_count"], 0);
    assert_eq!(cleared["failed_count"], 0);
    assert!(
        cleared["last_error"].is_null()
            && cleared["rule_name"].is_null()
            && cleared["next_attempt_at"].is_null()
    );
    let contents = tokio::time::timeout(std::time::Duration::from_secs(10), server).await??;
    let id = |msg: &String| {
        msg.lines()
            .find(|line| line.to_ascii_lowercase().starts_with("message-id:"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(id(&contents[0]), id(&contents[1]));
    let message = mailparse::parse_mail(contents[1].as_bytes())?;
    let delivered_body = message.get_body()?;
    assert!(delivered_body.contains("Live 1") && delivered_body.contains("Live 2"));
    assert!(!contents[1].to_lowercase().contains("bcc:"));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM mail_batches WHERE state='sent' AND attempts=2",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn explicit_test_upgrades_with_starttls_before_message_data() -> Result<()> {
    let (port, ca, server) = server(true, vec![true]).await?;
    let tmp = TempDir::new()?;
    let mut service = MailService::new(tmp.path().into());
    service.test_ca = Some(ca);
    let app = service.router();
    assert_eq!(
        request(&app, "POST", "/api/mail", configuration(port, "starttls"))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "POST", "/api/mail/test", json!({})).await.0,
        StatusCode::OK
    );
    let contents = server.await?;
    assert!(contents[0].contains("[Test]"));
    Ok(())
}

#[tokio::test]
async fn required_starttls_rejects_downgrade_before_mail_or_auth() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut io = BufReader::new(socket);
        reply(&mut io, b"220 local\r\n").await;
        assert!(line(&mut io).await.starts_with("EHLO "));
        reply(&mut io, b"250 local\r\n").await;
        let next = line(&mut io).await;
        assert!(next.is_empty() || next == "QUIT\r\n");
        if !next.is_empty() {
            reply(&mut io, b"221 Bye\r\n").await;
        }
    });
    let tmp = TempDir::new()?;
    let app = MailService::new(tmp.path().into()).router();
    assert_eq!(
        request(&app, "POST", "/api/mail", configuration(port, "starttls"))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "POST", "/api/mail/test", json!({})).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    peer.await?;
    Ok(())
}

#[tokio::test]
async fn reboot_respects_expired_leases_window_limits_and_concurrent_claims() -> Result<()> {
    let (port, ca, server) = server(false, vec![true]).await?;
    let tmp = TempDir::new()?;
    let mut service = MailService::new(tmp.path().into());
    service.test_ca = Some(ca);
    let app = service.router();
    assert_eq!(
        request(&app, "POST", "/api/mail", configuration(port, "tls"))
            .await
            .0,
        StatusCode::OK
    );
    let path = tmp.path().join("test.sqlite3");
    let db = Connection::open(&path)?;
    migrate(&db)?;
    let policy = MailPolicy {
        mode: MailMode::Window,
        schedule: Schedule {
            timezone: "UTC".into(),
            windows: vec![Window {
                days: vec![1],
                start: "09:00".into(),
                end: "10:00".into(),
            }],
        },
        digest_times: vec![],
    };
    let monday = utc("2026-01-05T09:00:00Z");
    enqueue(
        &db,
        "a",
        "Règle A",
        "Bloodborne",
        &stream("12"),
        &policy,
        monday - Duration::days(2),
    )?;
    let rules = vec!["a".into()];
    assert!(
        !service
            .dispatch_next(&path, monday - Duration::seconds(1), &rules)
            .await?
    );
    // Represent a process that stopped after claiming but before its reply.
    db.execute(
        "UPDATE mail_batches SET state='sending', attempts=1, next_attempt=?",
        [(monday + Duration::minutes(10)).timestamp()],
    )?;
    assert!(
        !service
            .dispatch_next(&path, monday + Duration::minutes(9), &rules)
            .await?
    );
    assert!(
        service
            .dispatch_next(&path, monday + Duration::hours(1), &rules)
            .await?
    );
    let next = monday + Duration::days(7);
    assert!(
        !service
            .dispatch_next(&path, next - Duration::seconds(1), &rules)
            .await?
    );
    let (a, b) = tokio::join!(
        service.dispatch_next(&path, next, &rules),
        service.dispatch_next(&path, next, &rules)
    );
    assert_ne!(a?, b?);
    assert_eq!(server.await?.len(), 1);
    assert_eq!(
        db.query_row(
            "SELECT attempts FROM mail_batches WHERE state='sent'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        2
    );
    Ok(())
}

#[tokio::test]
async fn deletion_during_smtp_preserves_accepted_mail_and_discards_refused_mail() -> Result<()> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("outbox.sqlite");
    let db = Connection::open(&db_path)?;
    migrate(&db)?;
    let mut names = ["accept", "refuse"].into_iter();
    let during = db_path.clone();
    let (port, certificate, sink) = server_during_data(false, vec![true, false], move || {
        super::super::retire_rule(&during, names.next().unwrap()).unwrap();
    })
    .await?;
    let mut service = MailService::new(dir.path().into());
    service.test_ca = Some(certificate);
    let app = service.router();
    assert_eq!(
        request(&app, "POST", "/api/mail", configuration(port, "tls"))
            .await
            .0,
        StatusCode::OK
    );
    let now = Utc::now();
    for id in ["accept", "refuse"] {
        enqueue(
            &db,
            id,
            id,
            "Catégorie",
            &stream(id),
            &MailPolicy::default(),
            now,
        )?;
        assert!(
            service
                .dispatch_next(&db_path, now, &[id.to_string()])
                .await?
        );
    }
    assert_eq!(sink.await?.len(), 2);
    assert_eq!(
        db.query_row("SELECT count(*) FROM mail_batches", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT rule_id FROM mail_batches WHERE state='sent'",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "accept"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM mail_events", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}
