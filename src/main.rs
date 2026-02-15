// MSX Term
// Copyright (c) 2023 Akio Setsumasa 
// Released under the MIT license
// https://github.com/akio-se/msxterm
//

// 初期開発中は Warnning 抑制
#![allow(unused_variables)]
#![allow(dead_code)]
//
mod msxcode;
mod connection;

use std::net::{Shutdown, TcpStream};
use std::thread;

use rustyline::config::Configurer;
//use std::sync::Arc;
use rustyline::{DefaultEditor, EditMode, ExternalPrinter, Result, error::ReadlineError};
use std::collections::{BTreeMap, HashMap};
use clap::Parser;
use std::fs::File;
use std::io::{BufRead, Write, BufReader,BufWriter};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
//use serialport::{ SerialPort, SerialPortType, available_ports};
use serial2::{SerialPort};
//use crate::connection::{TcpConnection, SerialConnection};

const C_CR: char = '\u{000d}';
const C_LF: char = '\u{000a}';

const U_BREAK:u8 = 0x03;
const U_BS:u8 = 0x08;
const U_LF:u8 = 0x0a;
const U_CR:u8 = 0x0d;
const U_PAUSE:u8 = 0x7b;

fn dump_hex(uv: Vec<u8>) -> String
{
    let mut cv:String = "".to_string();
    for u in uv {
        let tmp = format!("{:02X} ", u);
        cv.push_str(tmp.as_str());
    }
    cv
}

#[test]
fn test_dump_hex() {
    let uv: Vec<u8> = [0x41,0x51,0x61,0x71,0x80,0x81,0x8A,0xB3,0xC4,0x55].to_vec();
    let s = dump_hex(uv);
    println!("{}",s);
    assert!(s == "41 51 61 71 80 81 8A B3 C4 55 ");
}


#[test]
fn test_hex () {
    let s = "#HEX 40 41 42 43 44";
    let v = hex2u8(s);
    println!("{:?}", v);
}

fn hex2u8(hex: &str) -> Vec<u8> {
    let mut hex_vec: Vec<u8> = Vec::new();
    let tokens: Vec<&str> = hex.split(' ').collect();
    for token in &tokens[1..] {
        if let Ok(val) = u8::from_str_radix(token, 16) {
            hex_vec.push(val)   
        }
    }
    hex_vec
}

//
// 指定されたファイルをロードしてvec<String>を返す
//
fn load(command_line: &str) -> Result<Vec<String>> {
    let tokens: Vec<&str> = command_line.split(' ').collect();
    let path_str = tokens[1];
    // ファイルのパス
    let path = PathBuf::from(path_str.trim_matches('\"'));
    let file = File::open(path)?;
    let reader = BufReader::new(file);       
    let mut lines = Vec::new();
    for line in reader.lines() {
        lines.push(line?);
    }
    Ok(lines)
}

//
// コマンドラインオプションの設定
//
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Host_IP or Serial_Port
    target: Option<String>,

    /// history file name
    #[arg(short, long, value_name = "history file", default_value = "history.txt")]
    file: String,

    #[arg(short, long, value_name= "emacs or vi", default_value = "emacs")]
    editor: Option<String>,

    /// Use Serial Port
    #[arg(short, long)]
    serial: bool,

    /// Display Serial Port List
    #[arg(short, long)]
    port_list: bool,
}

#[derive(Clone)]
struct Msxterm {
    dump_mode: bool,
    lower_mode: bool,
    kanji_mode: bool,
    prog_buff:BTreeMap<u16, String>,
    t_com: HashMap<String, String>,
}

impl Msxterm {
    pub fn new() -> Msxterm {
        Msxterm { 
            dump_mode: false, 
            lower_mode: false,
            kanji_mode: false,
            prog_buff: BTreeMap::new(), 
            t_com: HashMap::new(),
        }
    }
    fn init(&mut self) {
        self.t_com.insert("#list".to_string(), "list Program".to_string());
    }

    pub fn parse_basic(&mut self, line:&str) {
        let mut iter = line.splitn(2,' ');
        if let Some(number) = iter.next() {
            if let Ok(number) = number.parse::<u16>() {
                if let Some(instruction) = iter.next() {
                    self.prog_buff.insert(number, instruction.trim().to_owned());
                } else {
                    self.prog_buff.remove(&number);
                }
            }
        }
    }

    pub fn print_basic(&self, start:u16, end:u16) -> Vec<String> {
        // println!("list {} {}", start, end);
        let mut history = Vec::new();
        if let Some(maxline) = self.prog_buff.iter().max() {
            let maxlen = maxline.0.to_string().len();
            let iter = self.prog_buff.range(start..=end);
            //for (num, inst) in &self.prog_buff {
            for (num ,inst) in iter {
                let padding = " ".repeat(maxlen - num.to_string().len());
                // Return formatted string instead of printing directly
                // Original format for history was: "{num} {inst}" (without padding)
                history.push(std::format!("{} {}", num, inst ));
            }
        }
        history
    }

    pub fn clear_basic(&mut self) {
        self.prog_buff.clear();
    }

    pub fn save_program(&self, command_line:&str) {
        let tokens: Vec<&str> = command_line.split(' ').collect();
        let path_str = tokens[1];
        // ファイルのパス
        let path = PathBuf::from(path_str.trim_matches('\"'));
        // ファイルを作成する
        let file = File::create(path).expect("Failed to create file");
        // ファイルに書き込むためのBufWriterを作成する
        let mut writer = BufWriter::new(file);

        // BTreeMapを文字列に変換してファイルに書き込む
        for (line_number, program) in self.prog_buff.iter() {
            let line = format!("{} {}\n", line_number, program);
            writer.write_all(line.as_bytes()).expect("Failed to write to file");
        }
        // ファイルをクローズする
        writer.flush().expect("Failed to flush buffer");
    }    


}

pub fn parse_command(command: &str) -> (Option<u16>, Option<u16>) {
    let mut parts = command.trim().split(' ');
    let _ = parts.next(); // Skip the command name
    let range = parts.next();
    match range {
        None => (None, None),
        Some(range) => {
            let mut range_parts = range.split('-');
            let start = range_parts.next().and_then(|x| x.parse().ok());
            let end = range_parts.next().and_then(|x| x.parse().ok());
            (start, end)
        }
    }
}

enum Command {
    DumpModeOn,
    DumpModeOff,
    KanjiModeOn,
    KanjiModeOff,
    ReloadFromStart,
    ReloadFromStop,
    RenumStart,
    ConditionalCommandStart,
}

// Parse a BASIC line in the format "linenum statement" and return (line_number, statement)
fn parse_basic_line(line: &str) -> Option<(u16, String)> {
    let trimmed = line.trim_start(); // Only trim leading whitespace
    if trimmed.is_empty() {
        return None;
    }
    
    // Find the first space to separate line number from the statement
    if let Some(pos) = trimmed.find(' ') {
        let line_num_str = &trimmed[..pos];
        let content = &trimmed[pos + 1..]; // Everything after the first space
        
        // Try to parse the line number part
        if let Ok(line_num) = line_num_str.parse::<u16>() {
            Some((line_num, content.to_string()))
        } else {
            None
        }
    } else {
        None
    }
}

#[test]
fn test_msxterm () {
    let mut mt = Msxterm::new();
    mt.init();

    let basfile = load("#load ./src/test.bas").unwrap();
    for s in basfile {
        mt.parse_basic(&s);
    }
    mt.print_basic(0,65530);

    let (st,ed) = parse_command("#list 10-20");
    println!("list {}-{}", st.unwrap_or(0), ed.unwrap_or(65530));

    let (st,ed) = parse_command("#list 40-");
    println!("list {}-{}", st.unwrap_or(0), ed.unwrap_or(65530));

    let (st,ed) = parse_command("#list -50");
    println!("list {}-{}", st.unwrap_or(0), ed.unwrap_or(65530));

    let (st,ed) = parse_command("#list 50");
    println!("list {}-{}", st.unwrap_or(0), ed.unwrap_or(65530));

    /*
    mt.parse_basic("1000 print 10 + 20");
    mt.parse_basic("100 cls");
    mt.parse_basic("1010 goto 1000");
    mt.parse_basic("20010 gosub 1000");
    mt.print_basic(0,65530);
    mt.parse_basic("100");
    mt.parse_basic("#list");
    mt.print_basic(0,65530);

    mt.parse_basic("1010 for i=0 to 100");
    mt.parse_basic("1020 PRINT I");
    mt.parse_basic("1030 NEXT I");
    mt.parse_basic("20010 gosub 1000");
    mt.print_basic(2000, 65530);
    */
}

#[test]
fn test_parse_basic_line() {
    assert_eq!(parse_basic_line("10 PRINT \"HELLO\""), Some((10, "PRINT \"HELLO\"".to_string())));
    assert_eq!(parse_basic_line("100 FOR I=1 TO 10"), Some((100, "FOR I=1 TO 10".to_string())));
    assert_eq!(parse_basic_line("  50 CLS  "), Some((50, "CLS  ".to_string())));  // Keeping trailing spaces as in original
    assert_eq!(parse_basic_line("PRINT \"NO NUMBER\""), None);
    assert_eq!(parse_basic_line(""), None);
}

fn lower_program(input:&str) -> String {
    let mut output = String::new();
    let mut is_quoted = false;

    for line in input.lines() {
        let mut tmp="".to_string();
        let tline = line.trim();
        for c in tline.chars() { 
            if c == '"' {
                is_quoted = !is_quoted;
            }
            if is_quoted {
                tmp.push(c);
            } else {
                let lowercase = c.to_lowercase().next().unwrap();
                tmp.push(lowercase);
            }
        }
        if tmp.starts_with("rem") {
            output.push_str(line.trim());
            output.push(C_CR);
        } else {
            output.push_str(&tmp);
            output.push(C_CR);
        }
    }
    output
}

#[test]
fn test_lower_program() {
    let text = "input text \"PrintHello\"
                        REM Akio Setsumasa
                        Print A$ + B$
                        REM This Program is Free
                      END";
    println!("{}", text);
    let result = lower_program(text);
    println!("{}", result);
}

fn serial_port_list() {
    // シリアルポートの情報を取得する
    let ports = SerialPort::available_ports().expect("Failed to get serial port list");

    // USB接続されたシリアルポートを検索する
    for port in ports {
        let str = port.into_os_string().into_string().unwrap();
        println!("USB Serial Port found: {}", str);
    }
}


fn main() -> Result<()> {
    // 変数初期化
    let mut msxterm_local = Msxterm::new();
    msxterm_local.init();

    // コマンドライン引数取得
    let args = Args::parse();
    if args.port_list {
        serial_port_list();
        return Ok(());
    }
    let target = match args.target {
        Some(target) => {
            target
        },
        _ => {
             "".to_string()
         }
    };
/*
    println!("file {}", args.file);
    println!("serial {}", args.serial);
    println!("portlist {}", args.port_list);
*/
    // エディタを生成
    let mut rl = DefaultEditor::new()?;
    match args.editor {
        Some(ed) =>  {
            if ed.eq("emacs") {
                rl.set_edit_mode(EditMode::Emacs);
            } else if ed.eq("vi") {
                rl.set_edit_mode(EditMode::Vi);
            }
        },
        _ => {}
    }
    
    // F4 and F5 key bindings
    use rustyline::Cmd;
    use rustyline::KeyEvent;
    use rustyline::KeyCode;
    use rustyline::Modifiers;
    
    // F4 key - insert and execute "list" command
    rl.bind_sequence(KeyEvent(KeyCode::F(4), Modifiers::NONE), 
        Cmd::Insert(1, "list".to_string()));
    
    // F5 key - insert and execute "run" command  
    rl.bind_sequence(KeyEvent(KeyCode::F(5), Modifiers::NONE), 
        Cmd::Insert(1, "run".to_string()));
        
    // Shift+F4 key - insert "#list" command
    rl.bind_sequence(KeyEvent(KeyCode::F(4), Modifiers::SHIFT), 
        Cmd::Insert(1, "#list".to_string()));
        
    // Shift+F2 key - insert "#load" command
    rl.bind_sequence(KeyEvent(KeyCode::F(2), Modifiers::SHIFT), 
        Cmd::Insert(1, "#load".to_string()));

    let mut printer = rl.create_external_printer()?;
    if rl.load_history(&args.file).is_err() {
        println!("No previous history.");
    }

    // ソケットを接続
    let server_address = target.clone();
    println!("Connecting... {}", server_address);

/*
    let mut conn = connection::create_connection(&target);
    let mut conn_read  = Arc::new(Mutec::new(conn));
    let mut conn_write = Arc::new(Mutec::new(conn));
*/

    let mut stream;
    let r = TcpStream::connect(server_address);
    match r {
        Ok(s) => {
            stream = s;
            println!("connected.");
        },
        Err(_) => {
            eprintln!("Failed to connect.");
            return Ok(());
        }, 
    }

    // 通信スレッドとメインスレッド間でやりとりするチャンネルを作成する
    let (tx, rx): (Sender<Command>, Receiver<Command>) = channel();
    
    // reload_from用の共有データ構造
    use std::sync::{Arc, Mutex};
    let shared_msxterm = Arc::new(Mutex::new(msxterm_local));
    let shared_msxterm_clone = shared_msxterm.clone();
    
    // 受信用スレッドを作成
    let stream_clone = stream.try_clone().expect("Failed to clone stream");
    let command_stream_for_receive_thread = stream.try_clone().expect("Failed to clone stream for receive thread commands");
    let receive_thread = thread::spawn(move || {
        let mut dump_mode = false;
        let mut kanji_mode = false;
        let mut reload_from_active = false; // Flag to indicate reload_from is active
        let mut renum_sequence_active = false; // Flag to indicate we're in a renum->list sequence
        let mut expect_listing_after_ok = false; // Flag to indicate we should start listing after "Ok" from renum
        let mut has_received_program_lines = false; // Flag to track if we've started receiving program lines
        let mut syntax_error_detected = false; // Flag to track if a syntax error was detected in the current operation
        let pending_command: Option<String> = None; // Command to execute if no error occurs
        let mut command_stream = command_stream_for_receive_thread;
        let mut reader = std::io::BufReader::new(&stream_clone);
        loop {
            if let Ok(command) = rx.recv_timeout(Duration::from_millis(1)) {
                match command {
                    Command::DumpModeOn => dump_mode = true,
                    Command::DumpModeOff => dump_mode = false,
                    Command::KanjiModeOn => kanji_mode = true,
                    Command::KanjiModeOff => kanji_mode = false,
                    Command::ReloadFromStart => {
                        reload_from_active = true;
                        syntax_error_detected = false; // Reset error flag for new operation
                    },
                    Command::ReloadFromStop => reload_from_active = false,
                    Command::RenumStart => {
                        renum_sequence_active = true;
                        expect_listing_after_ok = true;
                        reload_from_active = false; // Don't start capturing yet
                        syntax_error_detected = false; // Reset error flag for new operation
                    },
                    Command::ConditionalCommandStart => {
                        // This would be used to indicate we're waiting for a response
                        // before executing a pending command
                        syntax_error_detected = false; // Reset error flag
                    },
                }
            }
            let mut byte_buff: Vec<u8> = [0x00_u8; 0].to_vec();
            let result = reader.read_until(U_LF, &mut byte_buff);
            match result {
                Ok(size) => {
                    if size == 0 {
                        printer.print("Tcp disconnect".to_string()).expect("External print failure");
                        break;
                    }
                },
                Err(e) => {
                    printer.print(e.to_string()).expect("External print failure");
                    break;
                }
            }
            if dump_mode {
                let recv_buff = dump_hex(byte_buff);
                printer.print(recv_buff).expect("External print failure");
            } else {
                let recv_buff_raw = if kanji_mode {
                    msxcode::msx_kanji_to_string(byte_buff.clone())
                } else {
                    msxcode::msx_ascii_to_string(byte_buff.clone())
                };
                
                // If reload_from is active, parse the received lines as program listings
                if reload_from_active {
                    // Remove ANSI color codes and clean up the string
                    let clean_recv_buff = strip_ansi_escapes::strip(&recv_buff_raw);
                    let recv_buff = String::from_utf8_lossy(&clean_recv_buff);
                    
                    // Check if this looks like a program line (starts with a number)
                    let trimmed = recv_buff.trim();
                    if !trimmed.is_empty() {
                        // Check for various error messages - if found, stop the reload process
                        if trimmed.to_lowercase().contains("syntax error") ||
                           trimmed.to_lowercase().contains("out of memory") ||
                           trimmed.to_lowercase().contains("overflow") ||
                           trimmed.to_lowercase().contains("next without for") ||
                           trimmed.to_lowercase().contains("return without gosub") ||
                           trimmed.to_lowercase().contains("undefined line") ||
                           trimmed.to_lowercase().contains("illegal function call") ||
                           trimmed.to_lowercase().contains("string too long") {
                            // Stop the reload process due to error and clear the buffer
                            reload_from_active = false;
                            has_received_program_lines = false;
                            syntax_error_detected = true; // Mark that a syntax error occurred
                            // Clear the program buffer since renumbering failed
                            if let Ok(mut msxterm_guard) = shared_msxterm_clone.lock() {
                                msxterm_guard.clear_basic();
                            }
                            printer.print("Syntax Error detected. Stopping reload operation and clearing buffer.".to_string()).expect("External print failure");
                        }
                        // Check if this is the end of the listing (some indication like "Ok" or empty line)
                        // For now, we'll assume that if the line contains "Ok" or looks like a command prompt, it's the end
                        // BUT only end if we have already received some program lines (to avoid ending on "Ok" from renum)
                        // Also don't end if a syntax error was detected
                        else if (trimmed == "Ok" || trimmed.ends_with("OK") || trimmed.contains("READY") || trimmed.contains("ok")) && has_received_program_lines && !syntax_error_detected {
                            reload_from_active = false;
                            has_received_program_lines = false; // Reset for next time
                            // Don't reset syntax_error_detected here since it was already false
                            printer.print("Program reload complete.".to_string()).expect("External print failure");
                        } else if (trimmed == "Ok" || trimmed.ends_with("OK") || trimmed.contains("READY") || trimmed.contains("ok")) && has_received_program_lines && syntax_error_detected {
                            // If end marker is received but syntax error was detected, still end the reload but keep error flag
                            reload_from_active = false;
                            has_received_program_lines = false; // Reset for next time
                            // Keep syntax_error_detected as true until next operation starts
                            printer.print("Program reload complete (after error detected).".to_string()).expect("External print failure");
                        } else {
                            // Try to parse as a BASIC line number followed by content
                            if let Some((line_num, content)) = parse_basic_line(trimmed) {
                                if let Ok(mut msxterm_guard) = shared_msxterm_clone.lock() {
                                    msxterm_guard.prog_buff.insert(line_num, content.clone());
                                    // Mark that we've received program lines
                                    has_received_program_lines = true;
                                    // Print the line as it's received
                                    printer.print(format!("{} {}", line_num, content)).expect("External print failure");
                                }
                            } else {
                                // If it doesn't look like a program line, just print normally
                                printer.print(recv_buff_raw).expect("External print failure");
                            }
                        }
                    }
                } else if renum_sequence_active && expect_listing_after_ok {
                    // Special handling for renum sequence: wait for "Ok" from renum, then start capturing listing
                    let clean_recv_buff = strip_ansi_escapes::strip(&recv_buff_raw);
                    let recv_buff = String::from_utf8_lossy(&clean_recv_buff);
                    
                    let trimmed = recv_buff.trim();
                    if !trimmed.is_empty() {
                        // Check for various error messages - if found, stop the renum sequence
                        if trimmed.to_lowercase().contains("syntax error") ||
                           trimmed.to_lowercase().contains("out of memory") ||
                           trimmed.to_lowercase().contains("overflow") ||
                           trimmed.to_lowercase().contains("next without for") ||
                           trimmed.to_lowercase().contains("return without gosub") ||
                           trimmed.to_lowercase().contains("undefined line") ||
                           trimmed.to_lowercase().contains("illegal function call") ||
                           trimmed.to_lowercase().contains("string too long") {
                            // Stop the renum sequence and reload process due to error
                            renum_sequence_active = false;
                            expect_listing_after_ok = false;
                            reload_from_active = false;
                            has_received_program_lines = false;
                            syntax_error_detected = true; // Mark that a syntax error occurred
                            // DO NOT clear the program buffer since renumbering failed - preserve original content
                            printer.print("Syntax Error detected. Stopping renum operation. Buffer preserved.".to_string()).expect("External print failure");
                        }
                        // If this is the "Ok" response from the renum command, start expecting the listing
                        // But only if no syntax error was detected
                        else if (trimmed == "Ok" || trimmed == "OK" || trimmed == "ok" || 
                           trimmed.contains("OK.")) && !syntax_error_detected {
                            // Now start the reload process to capture the listing from the subsequent list command
                            reload_from_active = true;
                            expect_listing_after_ok = false;
                            has_received_program_lines = false; // Reset the flag for this reload session
                            // Clear the program buffer now that renum was successful
                            if let Ok(mut msxterm_guard) = shared_msxterm_clone.lock() {
                                msxterm_guard.clear_basic();
                            }
                            println!("Program buffer cleared. Reloading renumbered program from MSX0...");
                            
                            // Send the list command to MSX0 to get the renumbered program
                            let list_cmd = format!("list\r");
                            if let Err(e) = command_stream.write(list_cmd.as_bytes()) {
                                eprintln!("Error sending list command: {}", e);
                            }
                            
                            // Print the "Ok" response
                            printer.print(recv_buff_raw).expect("External print failure");
                        } else if (trimmed == "Ok" || trimmed == "OK" || trimmed == "ok" || 
                           trimmed.contains("OK.")) && syntax_error_detected {
                            // If "Ok" is received but a syntax error was detected, just print and reset
                            printer.print(recv_buff_raw).expect("External print failure");
                            syntax_error_detected = false; // Reset error flag since we're done with this operation
                        } else {
                            // If it's not the expected "Ok", treat as normal
                            printer.print(recv_buff_raw).expect("External print failure");
                        }
                    }
                } else {
                    // Normal printing behavior
                    printer.print(recv_buff_raw).expect("External print failure");
                }
            }
        }
    });

    // エディタ入力とコマンド送信のメインループ
    'input:loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(tmpl) => {
                //let mut line_tmp: &str = line.as_str();
                let b = tmpl.as_str().replace("\r\n","\r").replace('\n',"\r");
                let lines: Vec<&str> = b.split(C_CR).collect();
                for line in lines {
                    rl.add_history_entry(line)?;

                    if line.starts_with("#quit") {
                        // TCP 接続終了
                        stream.shutdown(Shutdown::Both).expect("Shutdown Error");
                        break 'input;
                    }
                    if line.starts_with("#hex") {
                        let hex = hex2u8(line);
                        stream.write(&hex).expect("Failed to write to server");
                        continue;
                    }
                    if line.starts_with("#dump_on") {
                        tx.send(Command::DumpModeOn).expect("Thread sync Error");
                        println!("Output dump mode On");
                        continue;             
                    }
                    if line.starts_with("#dump_off") {
                        tx.send(Command::DumpModeOff).expect("Thread sync Error");
                        println!("Output dump mode Off");
                        continue;             
                    }
                    if line.starts_with("#lowsend_on") {
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.lower_mode = true;
                        }
                        println!("Lower Case send mode On");
                        continue;
                    }
                    if line.starts_with("#lowsend_off") {
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.lower_mode = false;
                        }
                        println!("Lower Case send mode Off");
                        continue;
                    }
                    if line.starts_with("#kanji_on") {
                        tx.send(Command::KanjiModeOn).expect("Thread sync Error");
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.kanji_mode = true;
                        }
                        println!("Kanji mode On");
                        continue;
                    }
                    if line.starts_with("#kanji_off") {
                        tx.send(Command::KanjiModeOff).expect("Thread sync Error");
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.kanji_mode = false;
                        }
                        println!("Kanji mode Off");
                        continue;
                    }
                    if line.starts_with("#emacs") {
                        rl.set_edit_mode(EditMode::Emacs);
                        continue;
                    }
                    if line.starts_with("#vi") {
                        rl.set_edit_mode(EditMode::Vi);
                        continue;
                    }
                    if line.starts_with("#clear_history") {
                        rl.clear_history().unwrap();
                        println!("History is cleared.");
                        continue;
                    }
                    if line.starts_with("#new") {
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.prog_buff.clear();
                        }
                        println!("Program Buffer is cleared.");
                        continue;
                    }
                    if line.starts_with("#load") {
                        match load(line) {
                            Ok(basic) => {
                                let mut ld_program = "".to_string();
                                for bl in basic {
                                    let mut tmp = bl.trim().to_string();
                                    // Access the shared Msxterm instance
                                    if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                                        msxterm_guard.parse_basic(tmp.as_str());
                                    }
                                    rl.add_history_entry(tmp.as_str())?;
                                    tmp.push(C_CR);
                                    ld_program.push_str(&tmp);
                                }
                                // Access the shared Msxterm instance
                                let lower_mode = if let Ok(msxterm_guard) = shared_msxterm.lock() {
                                    msxterm_guard.lower_mode
                                } else {
                                    false
                                };
                                
                                if lower_mode {
                                    ld_program = lower_program(&ld_program);
                                }
                                stream
                                .write(ld_program.as_bytes())
                                .expect("Failed to write to server");
                            },
                            Err(e) => {
                                println!("{}", e);
                            }
                        }
                        println!("Ok");
                        continue;
                    }
                    if line.starts_with("#list") {
                        let cols = line.split(' ');
                        // Access the shared Msxterm instance
                        if let Ok(msxterm_guard) = shared_msxterm.lock() {
                            // Calculate padding for alignment like the original print_basic did
                            if let Some(maxline) = msxterm_guard.prog_buff.iter().max() {
                                let maxlen = maxline.0.to_string().len();
                                for (num, inst) in msxterm_guard.prog_buff.range(0..=65530) {
                                    let padding = " ".repeat(maxlen - num.to_string().len());
                                    // Print with cyan coloring for line number like original
                                    let formatted_line = format!("{}{}{}{} {}",
                                        padding,
                                        "\x1b[36m",  // Cyan color
                                        num,
                                        "\x1b[0m",   // Reset color
                                        inst
                                    );
                                    println!("{}", formatted_line);  // Print with alignment and coloring
                                    rl.add_history_entry(format!("{} {}", num, inst))?;  // Add to history without formatting
                                }
                            }
                        }
                        continue;
                    }
                    if line.starts_with("#reload_from") {
                        // Clear the current program buffer
                        if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.clear_basic();
                        }
                        println!("Program buffer cleared. Reloading from MSX0...");
                        
                        // Notify the receive thread to start capturing program listing
                        tx.send(Command::ReloadFromStart).expect("Thread sync Error");
                        
                        // Send the LIST command to MSX0 to get the current program
                        let list_cmd = format!("list\r");
                        stream.write(list_cmd.as_bytes()).expect("Failed to write LIST command");
                        continue;
                    }
                    if line.starts_with("#renum") {
                        // Extract parameters from the renum command (after removing the #renum prefix)
                        let params = line.strip_prefix("#renum").unwrap_or("").trim();
                        
                        println!("Renumbering program on MSX0...");
                        println!("Checking for errors before clearing local buffer...");
                        
                        // Notify the receive thread that we're starting a renum sequence
                        tx.send(Command::RenumStart).expect("Thread sync Error");
                        
                        // Send the RENUM command to MSX0 with parameters (without the # prefix)
                        let renum_cmd = if params.is_empty() {
                            format!("renum\r")
                        } else {
                            format!("renum {}\r", params)
                        };
                        stream.write(renum_cmd.as_bytes()).expect("Failed to write RENUM command");
                        
                        // The receive thread will handle conditional execution of reload_from
                        continue;
                    }
                    if line.starts_with("#save") {
                        // Access the shared Msxterm instance
                        if let Ok(msxterm_guard) = shared_msxterm.lock() {
                            msxterm_guard.save_program(line);
                        }
                        println!("Ok");
                        continue;
                    }

                    // Access the shared Msxterm instance
                    if let Ok(mut msxterm_guard) = shared_msxterm.lock() {
                        msxterm_guard.parse_basic(line);
                    }

                    let mut tmp2 = line.to_string();
                    tmp2.push(C_CR);
                    
                    // Access the shared Msxterm instance
                    let (lower_mode, kanji_mode) = if let Ok(msxterm_guard) = shared_msxterm.lock() {
                        (msxterm_guard.lower_mode, msxterm_guard.kanji_mode)
                    } else {
                        (false, false)
                    };
                    
                    if lower_mode {
                        tmp2 = lower_program(&tmp2);
                    }

                    let faces_code = if kanji_mode {
                        msxcode::utf8_to_msx_kanji(tmp2.as_str())
                    } else {
                        msxcode::utf8_msx_jp_code(tmp2.as_str())
                    };

                    stream
                        .write(&faces_code).expect("Failed to write");
                }
            }
            Err(ReadlineError::Interrupted) => {
                // break 送信
                let buf = vec![U_BREAK];
                stream.write(&buf).expect("Failed to write");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // BS 送信
                let buf = vec![U_PAUSE];
                stream.write(&buf).expect("Failed to write");
                continue;
            }
            Err(err) => {
                println!("Error: {err:?}");
                break;
            }
        }
    }
    // 受信スレッド終了
    receive_thread
        .join()
        .expect("Failed to join receive thread");

    // 履歴ファイル記録
    match rl.save_history(& args.file)
    {
        Ok(_) => {
            println!("history save to {}", args.file);
        },
        Err(e) => {
            println!("{}", e.to_string());
        }
    }
    Ok(())
}


