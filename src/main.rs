use std::{io::{Cursor, Read}, net::{Ipv4Addr, SocketAddrV4, TcpListener}, process::exit, sync::{Arc, Mutex}, thread::spawn};
use clap::Parser;
use pine_ipc::*;
use serde::{ser::SerializeSeq, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{base64::Base64, serde_as};
use tungstenite::{accept_hdr, handshake::server::{ErrorResponse, Request, Response}, Message, Utf8Bytes};

macro_rules! error_exit {
    ($msg:expr) => {
        {
            eprintln!($msg);
            exit(1);
        }
    };
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Target emulator name
    #[arg(long, default_value_t = String::from("pcsx2"))]
    target: String,

    /// Target emulator slot
    #[arg(long)]
    slot: Option<u16>,

    /// WebSocket server port
    #[arg(long, default_value_t = 58021)]
    port: u16,
}

fn main() {
    // Parse args
    let args = Args::parse();

    // Connect to PINE and verify connection
    let pine_connect = match args.target.as_str() {
        "pcsx2" => PINE::connect("pcsx2", args.slot.unwrap_or(28011), args.slot.is_none()),
        "rpcs3" => PINE::connect("rpcs3", args.slot.unwrap_or(28012), args.slot.is_none()),
        "duckstation" => PINE::connect("duckstation", args.slot.unwrap_or(28011), args.slot.is_none()),
        _ => PINE::connect(args.target.as_str(), match args.slot { Some(x) => x, None => error_exit!("Slot must be specified")}, false)
    };
    let mut pine = match pine_connect {
        Ok(x) => x,
        Err(err) => error_exit!("Failed to connect to PINE: {err}"),
    };
    {
        let mut batch = PINEBatch::new();
        batch.add(PINECommand::MsgStatus);
        match pine.send(&mut batch) {
            Err(err) => error_exit!("Failed to send message to PINE: {err}"),
            Ok(_) => println!("Connected to PINE"),
        }
    }
    let arc = Arc::new(Mutex::new(pine));
    
    // Start WebSocket server
    let addr = Ipv4Addr::new(127, 0, 0, 1);
    let socket = SocketAddrV4::new(addr, args.port);
    let server = TcpListener::bind(socket).unwrap();
    println!("Started WebSocket server on port {}", args.port);
    
    // Create thread for each new connection
    for stream in server.incoming() {
        let clone = Arc::clone(&arc);
        spawn(move || {
            let callback = |req: &Request, response: Response| {
                if req.uri().path() != "/" {
                    return Err(ErrorResponse::new(Some("Invalid path".to_string())));
                }
                println!("Received a new WebSocket connection");

                Ok(response)
            };
            let mut websocket = match accept_hdr(stream.unwrap(), callback) {
                Ok(handshake) => handshake,
                Err(_) => return
            };

            loop {
                let msg = match websocket.read() {
                    Ok(x) => x,
                    Err(_) => break,
                };

                let res = match msg {
                    Message::Text(text) => {
                        Message::Text(Utf8Bytes::from(match serde_json::from_str::<WSRequest>(&text) {
                            Err(err) => format!("Failed to parse command: {err}"),
                            Ok(req) => match req {
                                WSRequest::ExecuteCommand { cmd } => {
                                    let mut pine_batch = PINEBatch::new();
                                    pine_batch.add(cmd);

                                    let mut pine = clone.lock().unwrap();
                                    match pine.send(&mut pine_batch) {
                                        Err(err) => serde_json::to_string(&WSErrorResponse { error: format!("Failed to send command: {err}") }).unwrap(),
                                        Ok(mut res) => serde_json::to_string(&WSResponse { res: res.pop().unwrap() }).unwrap(),
                                    }
                                }

                                WSRequest::ExecuteBatch { batch } => {
                                    let mut pine_batch = PINEBatch::from_iter(batch);

                                    let mut pine = clone.lock().unwrap();
                                    let res = pine.send(&mut pine_batch);
                                    match res {
                                        Err(err) => serde_json::to_string(&WSErrorResponse { error: format!("Failed to send command: {err}") }).unwrap(),
                                        Ok(res) => serde_json::to_string(&WSBatchResponse { res: res}).unwrap(),
                                    }
                                }

                                WSRequest::WriteBuffer { address, buffer } => {
                                    let mut batch = PINEBatch::new();
                                    let len = buffer.len();
                                    let mut reader = Cursor::new(buffer);
                                    let mut buf: [u8; 8] = [0; 8];
                                    for i in (0..len).step_by(8) {
                                        let _ = reader.read_exact(&mut buf);
                                        let val = u64::from_le_bytes(buf);
                                        batch.add(PINECommand::MsgWrite64 { mem: address + i as u32, val: val })
                                    }

                                    let mut pine = clone.lock().unwrap();
                                    let res = pine.send(&mut batch);
                                    match res {
                                        Err(err) => serde_json::to_string(&WSErrorResponse { error: format!("Failed to send command: {err}") }).unwrap(),
                                        Ok(res) => serde_json::to_string(&WSBatchResponse { res: res }).unwrap(),
                                    }
                                }
                            },
                        }))
                    },

                    // TODO: implement this
                    // Message::Binary(bin) => {
                    //     let mut pine = clone.lock().unwrap();
                    //     let res = pine.send_raw(bin.as_slice());
                    //     match res {
                    //         Err(err) => Message::Text(format!("Failed to send command: {err}")),
                    //         Ok(res) => match res {
                    //             PINEResult::Fail => Message::Text(json!({"error": format!("Command returned failure code")}).to_string()),
                    //             PINEResult::Ok(buf) => Message::Binary(buf),
                    //         },
                    //     }
                    // },

                    Message::Close(_) => {
                        println!("WebSocket connection closed");
                        break;
                    },
                    
                    _ => {
                        continue;
                    }
                };

                if let Err(_) = websocket.send(res) {
                    // Break connection if message failed to send
                    break;
                }
            }
        });
    }
}

#[repr(u8)]
#[derive(Deserialize)]
#[serde(remote = "PINECommand", tag = "command")]
enum PINECommandDef {
    MsgRead8 { mem: u32 },
    MsgRead16 { mem: u32 },
    MsgRead32 { mem: u32 },
    MsgRead64 { mem: u32 },
    MsgWrite8 { mem: u32, val: u8 },
    MsgWrite16 { mem: u32, val: u16 },
    MsgWrite32 { mem: u32, val: u32 },
    MsgWrite64 { mem: u32, val: u64 },
    MsgVersion,
    MsgSaveState { sta: u8 },
    MsgLoadState { sta: u8 },
    MsgTitle,
    MsgID,
    MsgUUID,
    MsgGameVersion,
    MsgStatus,
    MsgUnimplemented
}

#[repr(u32)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "PINEStatus")]
enum PINEStatusDef {
    Running,
    Paused,
    Shutdown,
    Unknown
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PINEResponse", tag = "command")]
enum PINEResponseDef {
    ResRead8 { val: u8 },
    ResRead16 { val: u16 },
    ResRead32 { val: u32 },
    ResRead64 { val: u64 },
    ResWrite8,
    ResWrite16,
    ResWrite32,
    ResWrite64,
    ResVersion { version: String },
    ResSaveState,
    ResLoadState,
    ResTitle { title: String },
    ResID { id: String },
    ResUUID { uuid: String },
    ResGameVersion { version: String },
    ResStatus { #[serde(with = "PINEStatusDef")] status: PINEStatus },
    ResUnimplemented
}

#[derive(Serialize)]
struct WSResponse {
    #[serde(with = "PINEResponseDef")]
    res: PINEResponse
}

#[derive(Serialize)]
struct WSBatchResponse {
    #[serde(serialize_with = "serialize_vec_response")]
    res: Vec<PINEResponse>
}

#[derive(Serialize)]
struct WSErrorResponse {
    error: String
}

#[serde_as]
#[derive(Deserialize)]
#[serde(tag = "command")]
enum WSRequest {
    #[serde(rename = "execute_command")]
    ExecuteCommand { #[serde(with = "PINECommandDef")] cmd: PINECommand },

    #[serde(rename = "execute_batch")]
    ExecuteBatch { #[serde(deserialize_with = "deserialize_vec_command")] batch: Vec<PINECommand> },
    
    #[serde(rename = "write_buffer")]
    WriteBuffer { address: u32, #[serde_as(as = "Base64")] buffer: Vec<u8> }
}

fn deserialize_vec_command<'de, D>(deserializer: D) -> Result<Vec<PINECommand>, D::Error> where D: Deserializer<'de> {
    #[derive(Deserialize)]
    struct Wrapper(#[serde(with = "PINECommandDef")] PINECommand);

    let v = Vec::deserialize(deserializer)?;
    Ok(v.into_iter().map(|Wrapper(a)| a).collect())
}

fn serialize_vec_response<S>(x: &Vec<PINEResponse>, s: S) -> Result<S::Ok, S::Error> where S: Serializer {
    #[derive(Serialize)]
    struct Wrapper<'a>(#[serde(with = "PINEResponseDef")] &'a PINEResponse);

    let mut ser = s.serialize_seq(Some(x.len()))?;
    for item in x.iter() {
        ser.serialize_element(&Wrapper(item))?;
    }
    ser.end()
}
