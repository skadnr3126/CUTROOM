use crate::project::{control, library::Library};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    time::Duration,
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[derive(Deserialize)]
struct Request {
    token: String,
    command: control::Command,
}

fn handle(socket: &mut TcpStream, app: &AppHandle, token: &str) -> Result<Value, String> {
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    BufReader::new(&mut *socket)
        .take(1_048_577)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 || bytes.last() != Some(&b'\n') {
        return Err("잘못된 요청 길이입니다.".into());
    }
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| "요청 형식이 잘못되었습니다.")?;
    if request.token != token {
        return Err("인증에 실패했습니다.".into());
    }
    let library = app.state::<Library>();
    let store = match &request.command {
        control::Command::RegisterOutput { project_id, .. } => library.by_id(*project_id)?,
        _ => library.current()?,
    };
    control::execute(app, &store, request.command)
}

pub(crate) fn start(app: AppHandle, data: &Path) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let token = Uuid::new_v4().to_string();
    std::fs::create_dir_all(data)?;
    let mut file = tempfile::NamedTempFile::new_in(data)?;
    serde_json::to_writer(
        &mut file,
        &json!({"port":listener.local_addr()?.port(),"token":token}),
    )?;
    file.persist(data.join("editor-control.json"))?;
    std::thread::Builder::new()
        .name("editor-control".into())
        .spawn(move || {
            // One worker owns each request so that reading the timeline remains possible during export.
            for stream in listener.incoming().flatten() {
                let app = app.clone();
                let token = token.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let response = match handle(&mut stream, &app, &token) {
                        Ok(value) => json!({"result":value}),
                        Err(error) => json!({"error":error}),
                    };
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                    let _ = serde_json::to_writer(&mut stream, &response);
                    let _ = stream.write_all(b"\n");
                });
            }
        })?;
    Ok(())
}
