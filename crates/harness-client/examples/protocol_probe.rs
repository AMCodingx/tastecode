use harness_client::{ClientEvent, ClientHandle, ConnectionState, Endpoint};
use harness_protocol::{ProjectsListResult, Response, channel, method};
use serde_json::json;
use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let (client, events) = ClientHandle::start(Endpoint::from_environment()?)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut projects_request = None;
    let mut welcomed = false;

    while Instant::now() < deadline {
        let Ok(event) = events.try_recv() else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        match event {
            ClientEvent::StateChanged(ConnectionState::Open) => {
                client.request(
                    method::CLIENT_CAPABILITIES,
                    json!({ "previewCapture": false }),
                )?;
                projects_request = Some(client.request(method::PROJECTS_LIST, json!({}))?);
            }
            ClientEvent::Push(push) if push.channel == channel::SERVER_WELCOME => {
                welcomed = true;
            }
            ClientEvent::Response(Response::Success { id, result })
                if projects_request.as_deref() == Some(id.as_str()) =>
            {
                let projects: ProjectsListResult = serde_json::from_value(result)?;
                if !welcomed {
                    return Err("projects.list arrived before server.welcome".into());
                }
                println!("protocol v2 ready; {} project(s)", projects.projects.len());
                return Ok(());
            }
            ClientEvent::Response(Response::Failure { id, error })
                if projects_request.as_deref() == Some(id.as_str()) =>
            {
                return Err(format!("projects.list failed: {}", error.message).into());
            }
            _ => {}
        }
    }

    Err("timed out waiting for projects.list".into())
}
