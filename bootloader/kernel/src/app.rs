use core::{
    alloc::Layout,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{boxed::Box, fmt::format, format, string::String, vec::Vec};
use xmas_elf::{program, sections::SectionData, ElfFile};

use crate::{allocator::ALLOCATOR, framebuffer::FBShare, globals, interrupts::global_time_ms};

#[repr(C)]
pub struct Context<'a> {
    pub version: u8,
    pub start_time: u64,
    pub log: extern "C" fn(s: &str),
    pub pid: u64,
    pub fb: FBShare<'a>,
    pub calloc: extern "C" fn(usize, usize) -> *mut u8,
    pub cdalloc: extern "C" fn(*mut u8, usize, usize),
    pub store: &'a mut Option<Box<()>>,
    pub input: &'a globals::Input,
}
static mut none: Option<Box<()>> = None;
impl<'a> Context<'a> {
    pub fn new(
        log: extern "C" fn(s: &str),
        fb: FBShare<'a>,
        calloc: extern "C" fn(usize, usize) -> *mut u8,
        cdalloc: extern "C" fn(*mut u8, usize, usize),
        input: &'a globals::Input,
    ) -> Context<'a> {
        let x = Context {
            version: 1,
            start_time: global_time_ms(),
            log,
            pid: 0,
            fb,
            calloc,
            cdalloc,
            store: unsafe { &mut none },
            input,
        };

        return x;
    }
}

type FuncType = extern "C" fn(arg: &mut Context) -> i32;

pub struct App {
    pub code: Vec<u8>,
    pub func: FuncType,
    pub pid: u64,
    pub store: Option<Box<()>>,
}
impl App {
    pub fn new(code: &[u8], show: bool) -> App {
        let code = code.to_vec();
        let code = &code[..];

        // log::info!("loa efl");
        let elf = ElfFile::new(code).expect("elf");
        // log::info!("Elf file loaded at {:#p}", elf.input);
        // log::info!("{:#?}", elf.header);

        let mut min_virt = u64::MAX;
        let mut max_virt = 0;

        for program_header in elf.program_iter() {
            if program_header.mem_size() > 0 {
                min_virt = min_virt.min(program_header.virtual_addr());
                max_virt = max_virt.max(program_header.virtual_addr() + program_header.mem_size());
            }
        }

        min_virt = 0;
        let cap = max_virt as usize;
        use core::alloc::GlobalAlloc;
        let ptr = unsafe { ALLOCATOR.alloc(Layout::from_size_align_unchecked(cap, 4096)) };

        let mut owned_code: Vec<u8> = unsafe { Vec::from_raw_parts(ptr, 0, cap) }; //Vec::with_capacity((max_virt - min_virt) as usize);
        for i in min_virt..max_virt {
            owned_code.push(0);
        }

        for program_header in elf.program_iter() {
            // log::info!("{:?}", program_header);

            if let Ok(program::Type::Load) = program_header.get_type() {
                let mut byte = 0;
                let seg_offset = (program_header.virtual_addr() - min_virt) as usize;
                for e in (program_header.offset() as usize)
                    ..(program_header.offset() as usize + ((program_header.file_size()) as usize))
                {
                    owned_code[byte + seg_offset] = code[e];
                    byte += 1;
                }

                if let Some(rela) = elf.find_section_by_name(".rela.dyn") {
                    if let Ok(SectionData::Rela64(arr)) = rela.get_data(&elf) {
                        for r in &arr[0..] {
                            let off = r.get_offset() as usize;
                            let add = r.get_addend() as usize;
                            let typ = r.get_type() as usize;
                            if off >= program_header.virtual_addr() as usize
                                && off
                                    < program_header.virtual_addr() as usize
                                        + program_header.mem_size() as usize
                            {
                                // log::info!("{:?}", r);
                                unsafe {
                                    let global_off = owned_code.as_ptr() as u64;
                                    let p64 = owned_code.as_ptr().offset(off as isize) as *mut u64;
                                    p64.write(add as u64 + global_off);
                                }

                                // for k in 0..8 {
                                //     owned_code[off + k] = owned_code[add + k];
                                // }
                            }
                        }
                    }
                }
            }
        }

        // log::info!("{:?}", owned_code);
        let codep = unsafe {
            owned_code
                .as_mut_ptr()
                .offset((elf.header.pt2.entry_point() - min_virt) as isize)
        };
        // log::info!("ptr {:?}", codep);
        let codef: FuncType = unsafe { core::intrinsics::transmute(codep) };

        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        if show {
            let entry = (elf.header.pt2.entry_point() - min_virt) as usize;
            let code = &mut owned_code[entry..];
            use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
            let EXAMPLE_CODE_RIP = entry as u64;
            let HEXBYTES_COLUMN_BYTE_LENGTH = 8;
            let mut decoder = Decoder::with_ip(64, code, EXAMPLE_CODE_RIP, DecoderOptions::NONE);

            // Formatters: Masm*, Nasm*, Gas* (AT&T) and Intel* (XED).
            // For fastest code, see `SpecializedFormatter` which is ~3.3x faster. Use it if formatting
            // speed is more important than being able to re-assemble formatted instructions.
            let mut formatter = NasmFormatter::new();

            // Change some options, there are many more
            formatter.options_mut().set_digit_separator("`");
            formatter.options_mut().set_first_operand_char_index(10);

            // String implements FormatterOutput
            let mut output = String::new();

            // Initialize this outside the loop because decode_out() writes to every field
            let mut instruction = Instruction::default();

            // The decoder also implements Iterator/IntoIterator so you could use a for loop:
            //      for instruction in &mut decoder { /* ... */ }
            // or collect():
            //      let instructions: Vec<_> = decoder.into_iter().collect();
            // but can_decode()/decode_out() is a little faster:
            while decoder.can_decode() {
                // There's also a decode() method that returns an instruction but that also
                // means it copies an instruction (40 bytes):
                //     instruction = decoder.decode();
                decoder.decode_out(&mut instruction);

                log::info!("{:?}", instruction);

                // Format the instruction ("disassemble" it)
                output.clear();
                formatter.format(&instruction, &mut output);

                // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
                let mut strbuild = String::new();

                strbuild = format!("{}{:016X} ", strbuild, instruction.ip());
                let start_index = (instruction.ip() - EXAMPLE_CODE_RIP) as usize;
                let instr_bytes = &code[start_index..start_index + instruction.len()];
                for b in instr_bytes.iter() {
                    strbuild = format!("{}{:02X}", strbuild, b);
                }
                if instr_bytes.len() < HEXBYTES_COLUMN_BYTE_LENGTH {
                    for _ in 0..HEXBYTES_COLUMN_BYTE_LENGTH - instr_bytes.len() {
                        strbuild = format!("{}  ", strbuild);
                    }
                }
                log::info!("{} {}", strbuild, output);
            }
        }

        App {
            code: owned_code,
            func: codef,
            pid: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            store: None,
        }
    }
    pub fn call(&mut self, arg: &mut Context) -> i32 {
        *arg.store = None;

        arg.pid = self.pid;

        let self_store = self.store.take();

        *arg.store = self_store;
        let res = (self.func)(arg);

        self.store = arg.store.take();

        res
    }
}
