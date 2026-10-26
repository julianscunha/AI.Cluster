#!/usr/bin/env python3
"""Gera um script do u-boot (imagem legada, como `mkimage -T script`) sem depender do mkimage.

    python3 ubootscr.py entrada.txt saida.scr
"""
import struct
import sys
import time
import zlib


def uboot_script(text: str, name: str = 'aic') -> bytes:
    body = text.encode()
    data = struct.pack('>II', len(body), 0) + body
    now = int(time.time())  # uma leitura só: o CRC do cabeçalho cobre este campo
    header = lambda hcrc: struct.pack('>IIIIIIIBBBB32s', 0x27051956, hcrc, now, len(data), 0, 0,
                                      zlib.crc32(data), 5, 2, 6, 0, name.encode())
    return header(zlib.crc32(header(0))) + data


if __name__ == '__main__':
    open(sys.argv[2], 'wb').write(uboot_script(open(sys.argv[1], encoding='utf-8').read()))
