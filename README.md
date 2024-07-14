# pine-ws
WebSocket relay for the [PINE IPC protocol](https://github.com/GovanifY/pine) using [pine-rs](https://github.com/entriphy/pine-rs).

## Command Line
`pine-ws` accepts the following command line arguments:
* `--target`: The name of the emulator, i.e. `pcsx2`, `rpcs3`, `duckstation`. Default is `pcsx2`.
    * If `target` does not equal one of the specified emulators, `slot` must be specified.
* `--slot`: The IPC slot/port of the emulator. Default is 28011 if `target` is `pcsx2` or `duckstation`, 28012 if `target` is `rpcs3`.
* `--port`: The port to run the WebSocket server on. Default is `58012`.

## Sending Commands
Messages are sent between the client and server as JSON strings.

Request example:
```json
{
    "command": "execute_batch",
    "batch": [
        { "command": "MsgTitle" },
        { "command": "MsgGameVersion" }
        { "command": "MsgRead32", "mem": 3565532  }
    ]
}
```

Response example:
```json
{
    "res": [
        { "command": "ResTitle", "title": "Klonoa 2 - Lunatea's Veil" },
        { "command": "ResGameVersion", "version": "1.00" },
        { "command": "ResRead32", "val": 3566512 }
    ]
}
```

Error response example:
```json
{
    "error": "Failed to parse command: ..."
}
```

### Commands
* `execute_command(cmd: { command: PINECommand, ... }) -> PINEResponse`: Executes a single PINE command and returns the response. Additional parameters are required for certain PINE commands.
* `execute_batch(batch: { command: PINECommand, ... }[]) -> PINEResponse[]`: Executes a list of PINE commands and returns a list of responses for each command. Additional parameters are required for certain PINE commands.
* `write_buffer(address: number, buffer: string) -> PINEResponse[]`: Writes a Base64-encoded buffer to the specified memory address. This command creates a batch of `MsgWriteN` commands to write the buffer to memory.

#### PINE Commands
`pine-ws` currently only accepts the commands specified in the PINE standard:
```rust
enum PINECommand {
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

enum PINEResponse {
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
    ResStatus { status: PINEStatus },
    ResUnimplemented
}
```