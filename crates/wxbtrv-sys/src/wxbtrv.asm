; wxbtrv.sys — DOS character device driver for the Watkins Btrieve stack
; Assembled with NASM into a flat 16-bit binary (.sys format).
;
; Responsibilities:
;   1. Presents itself to DOS as character device "WXBTRV00"
;   2. On INIT: calls NTVDM RegisterModule to load wxbtrv.dll as our VDD
;   3. Hooks INT 7B via INT 21h AH=25h: when a DOS app makes a Btrieve call,
;      fires BOP to the registered VDD
;
; NTVDM BOP (Break-Out Point) mechanism:
;   The 4-byte sequence  C4 C4 58 <sub>  is the NTVDM "magic" that transitions
;   the CPU from 16-bit V86 mode into a Win32 VDD handler.
;
;     C4 C4 58 00  — RegisterModule: loads the VDD DLL
;                     ES:SI = null-terminated VDD DLL filename
;                     ES:DI = null-terminated VDDRegisterInit export name
;                     ES:BX = null-terminated VDDDispatch export name
;                     Returns: AX = VDD handle (0 = error), CF set on error
;
;     C4 C4 58 02  — VDDDispatch: calls VDDDispatch in the registered DLL
;                     AX = VDD handle (obtained from RegisterModule)
;                     DS:DX = pointer to BtrCallBlock (see vdd.rs for layout)
;                     Returns: AX = Btrieve status code
;
; INT 7B calling convention (DOS app -> us):
;     DS:DX = pointer to 28-byte BtrCallBlock (see vdd.rs for layout)
;     Returns: AX = Btrieve status code
;
; Implements the standard DOS Btrieve INT 7B BOP call convention.

bits 16
org 0

; ── Device header (18 bytes) ──────────────────────────────────────────────────
    dw  0xFFFF              ; next device link offset  (last in chain)
    dw  0xFFFF              ; next device link segment (last in chain)
    dw  0xC840              ; attributes: character device + IOCTL + open/close
    dw  strategy            ; offset of Strategy routine
    dw  interrupt           ; offset of Interrupt routine
    db  "WXBTRV00"          ; device name (exactly 8 bytes)

; ── Resident data ─────────────────────────────────────────────────────────────
req_hdr_ptr:    dd  0               ; ES:DI saved by Strategy
vdd_handle:     dw  0               ; VDD handle returned by RegisterModule

vdd_dll:    db  WXBTRV_DLL_PATH, 0  ; Full path to DLL — injected at build time
vdd_init:   db  "VDDRegisterInit", 0
vdd_disp:   db  "VDDDispatch", 0

; ── Strategy ──────────────────────────────────────────────────────────────────
strategy:
    mov  [cs:req_hdr_ptr],   bx
    mov  [cs:req_hdr_ptr+2], es
    retf

; ── Interrupt ─────────────────────────────────────────────────────────────────
interrupt:
    push ax
    push bx
    push cx
    push dx
    push si
    push di
    push ds
    push es

    ; Load request header pointer into ES:DI
    les  di, [cs:req_hdr_ptr]
    mov  al, [es:di + 2]           ; command code byte
    cmp  al, 0                     ; 0 = INIT
    jne  .set_done

    ; ── Command 0: INIT ───────────────────────────────────────────────────────
    ; Set DS = ES = CS so that ES:SI/DI/BX all address our resident strings.
    push cs
    pop  ds
    push ds
    pop  es

    mov  si, vdd_dll
    mov  di, vdd_init
    mov  bx, vdd_disp
    db   0xC4, 0xC4, 0x58, 0x00    ; RegisterModule(ES:SI, ES:DI, ES:BX)
                                    ; returns VDD handle in AX, CF set on error
    jc   .init_error

    ; Store VDD handle for use by INT 7B handler
    mov  [cs:vdd_handle], ax

    ; Hook INT 7B via INT 21h AH=25h (Set Interrupt Vector)
    ; DS is already set to CS
    mov  ah, 0x25
    mov  al, 0x7B
    mov  dx, int7b_handler
    int  0x21

    ; Report end-of-resident-code to DOS
    les  di, [cs:req_hdr_ptr]
    mov  word [es:di + 14], init_end
    mov  [es:di + 16], cs
    jmp  .set_done

.init_error:
    ; RegisterModule failed — clear bit14 of attrs (no IOCTL queries), resident=0,
    ; return DONE with error code 3 (unknown command) matching btrdrvr.sys behaviour.
    ; Do NOT set ERROR|0x0C — that triggers NTVDM "not suitable" dialog.
    les  di, [cs:req_hdr_ptr]
    mov  byte [es:di + 13], 0           ; clear unit count
    mov  word [es:di + 14], 0           ; resident end offset = 0 (no resident)
    mov  [es:di + 16], cs               ; resident end segment
    and  word [cs:4], 0x8FFF            ; clear attr bit14 (IOCTL queries) like btrdrvr
    or   word [es:di + 3], 0x8103       ; DONE | ERROR | code 3 (btrdrvr convention)
    jmp  .done

.set_done:
    les  di, [cs:req_hdr_ptr]
    or   word [es:di + 3], 0x0100       ; status word: DONE bit

.done:
    pop  es
    pop  ds
    pop  di
    pop  si
    pop  dx
    pop  cx
    pop  bx
    pop  ax
    retf

; ── INT 7B handler ────────────────────────────────────────────────────────────
; Fired by every Btrieve call from the DOS application.
; DS:DX already points to the BtrCallBlock — VDDDispatch reads it.
; BUG FIX: do NOT push/pop AX — VDDDispatch returns the Btrieve status in AX
; via setAX(); popping old AX would discard that return code.
; CX=3 matches btrdrvr.sys BOP calling convention.
int7b_handler:
    push ax
    push cx
    mov  ax, [cs:vdd_handle]
    mov  cx, 3
    db   0xC4, 0xC4, 0x58, 0x02    ; VDDDispatch(AX=handle)
    pop  cx
    pop  ax
    iret

; ── End of resident section ───────────────────────────────────────────────────
init_end:
