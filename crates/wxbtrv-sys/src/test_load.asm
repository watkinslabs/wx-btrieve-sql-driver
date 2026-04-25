; test_load.sys — minimal DOS device driver.
; During INIT, creates C:\WatkinsX\logs\sys_loaded.txt via INT 21h.
; If the file appears after running a DOS app, the DEVICE= line is working.

bits 16
org 0

; ── Device header ─────────────────────────────────────────────────────────────
    dw  0xFFFF
    dw  0xFFFF
    dw  0x8000              ; character device
    dw  strategy
    dw  interrupt
    db  "WXTEST00"

req_hdr_ptr: dd 0

marker_path: db "C:\WatkinsX\logs\sys_loaded.txt", 0
marker_text: db "wxbtrv.sys INIT ran", 13, 10

strategy:
    mov  [cs:req_hdr_ptr],   bx
    mov  [cs:req_hdr_ptr+2], es
    retf

interrupt:
    push ax
    push bx
    push cx
    push dx
    push ds

    les  bx, [cs:req_hdr_ptr]
    mov  al, [es:bx + 2]
    cmp  al, 0
    jne  .done

    ; Create marker file
    push cs
    pop  ds
    mov  dx, marker_path
    mov  cx, 0                  ; normal attributes
    mov  ah, 0x3C               ; INT 21h: Create File
    int  21h
    jc   .done                  ; skip write if create failed

    ; Write text
    mov  bx, ax                 ; file handle
    mov  dx, marker_text
    mov  cx, 21                 ; byte count
    mov  ah, 0x40               ; INT 21h: Write File
    int  21h

    ; Close file
    mov  ah, 0x3E
    int  21h

    ; Set resident end = right here (no resident code needed)
    les  bx, [cs:req_hdr_ptr]
    mov  word [es:bx + 14], init_end
    mov  [es:bx + 16], cs

.done:
    les  bx, [cs:req_hdr_ptr]
    or   word [es:bx + 3], 0x0100

    pop  ds
    pop  dx
    pop  cx
    pop  bx
    pop  ax
    retf

init_end:
