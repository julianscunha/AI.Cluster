#!/usr/bin/env python3
"""Servidor TFTP mínimo, só leitura, para o boot pela rede do worker (F15 J4/J5).

    python3 tftpd.py --allow IP[,IP...] [--boots N] [--root DIR] [--port 69] [--bind 0.0.0.0]

Serve arquivos de DIR (somente para dentro dele) por TFTP (RFC 1350) com as opções blksize e
tsize (RFC 2347-2349), que o u-boot usa para baixar o kernel e o initramfs em segundos. Existe
porque o Docker Desktop não encaminha bem o TFTP (a resposta sai de uma porta diferente da 69 e
o NAT a descarta); em Linux, o mesmo script roda num contêiner com `--network host`.

Só atende os IPs de --allow: qualquer outro recebe erro e fica no log. O IP de origem de um
datagrama UDP pode ser forjado dentro do mesmo segmento, então isto barra o curioso, não o
atacante; para isso, isole o boot pela rede numa VLAN própria (ver o ADR de reimage por rede).

--boots N limita quantas vezes cada IP recebe a imagem inteira (a conta fecha no dtb, o último
arquivo); depois disso o servidor o
recusa e o u-boot cai no cartão local. É o que torna a imagem da rede um slot em teste: passado
o prazo, o worker reinicia e volta ao slot local sozinho em vez de rebootar na rede para sempre.

Parar este processo é a alavanca de recuperação do boot pela rede: sem servidor, o u-boot desiste
e o worker sobe do cartão local.

shortcut: uma thread por transferência e sem janela deslizante (windowsize), o que basta para
um worker por vez; trocar por um servidor de verdade (dnsmasq, tftpd-hpa) para uma frota.
"""
import argparse
import os
import socket
import struct
import threading

RRQ, DATA, ACK, ERROR, OACK = 1, 3, 4, 5, 6
DELIVERED = {}  # imagens entregues por IP
TIMEOUT = 1.0
RETRIES = 30
BLKSIZE_MAX = 512  # blocos menores toleram melhor o EMAC do u-boot do H3 (travava no bloco ~200 com 1468)


def error(sock, addr, code, text):
    sock.sendto(struct.pack("!HH", ERROR, code) + text.encode() + b"\0", addr)


def serve(request, addr, root, allowed, boots):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(TIMEOUT)
    sock.bind(("", 0))
    try:
        if addr[0] not in allowed:
            error(sock, addr, 2, "acesso negado")
            print(f"{addr[0]}: NEGADO (fora de --allow)")
            return
        if boots and DELIVERED.get(addr[0], 0) >= boots:
            error(sock, addr, 2, "limite de boots atingido")
            print(f"{addr[0]}: NEGADO (limite de {boots} boot(s) atingido)")
            return
        fields = request[2:].split(b"\0")
        name = fields[0].decode(errors="replace")
        options = {
            fields[i].decode().lower(): fields[i + 1].decode()
            for i in range(2, len(fields) - 1, 2)
        }
        path = os.path.realpath(os.path.join(root, name.lstrip("/")))
        if os.path.commonpath([path, root]) != root or not os.path.isfile(path):
            error(sock, addr, 1, "arquivo nao encontrado")
            print(f"{addr[0]} {name}: nao encontrado")
            return
        size = os.path.getsize(path)
        block = 512
        reply = {}
        if "blksize" in options:
            block = max(8, min(int(options["blksize"]), BLKSIZE_MAX))
            reply["blksize"] = str(block)
        if "tsize" in options:
            reply["tsize"] = str(size)
        print(f"{addr[0]} {name}: {size} bytes, bloco {block}")

        def send_and_wait(packet, expect):
            for _ in range(RETRIES):
                sock.sendto(packet, addr)
                try:
                    while True:
                        data, peer = sock.recvfrom(1024)
                        if peer == addr and len(data) >= 4:
                            kind, number = struct.unpack("!HH", data[:4])
                            if kind == ACK and number == expect:
                                return True
                            if os.environ.get("TFTP_DEBUG"):
                                print(f"  ack fora de ordem: veio {kind}/{number}, esperava {expect}", flush=True)
                except socket.timeout:
                    if os.environ.get("TFTP_DEBUG"):
                        print(f"  timeout aguardando ack {expect}", flush=True)
                    continue
            return False

        if reply:
            body = b"".join(k.encode() + b"\0" + v.encode() + b"\0" for k, v in reply.items())
            if not send_and_wait(struct.pack("!H", OACK) + body, 0):
                return
        with open(path, "rb") as file:
            number = 1
            while True:
                chunk = file.read(block)
                if not send_and_wait(struct.pack("!HH", DATA, number) + chunk, number):
                    print(f"{addr[0]} {name}: sem resposta no bloco {number}")
                    return
                if len(chunk) < block:
                    break
                number = (number + 1) & 0xFFFF
        print(f"{addr[0]} {name}: concluido")
        # O dtb é o último arquivo do boot: contar antes o negaria no meio da mesma entrega.
        if name.endswith("/dtb") or name == "dtb":
            DELIVERED[addr[0]] = DELIVERED.get(addr[0], 0) + 1
    finally:
        sock.close()


SLOTS = threading.BoundedSemaphore(4)  # transferências simultâneas


def guarded(*args):
    try:
        serve(*args)
    except Exception as exc:  # pedido malformado (nome com NUL, opção inválida) não derruba nem vaza traceback
        print(f"{args[1][0]}: pedido recusado ({type(exc).__name__})", flush=True)
    finally:
        SLOTS.release()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=os.path.join(os.path.dirname(__file__), "srv"))
    parser.add_argument("--port", type=int, default=69)
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--allow", required=True, help="IPs que podem baixar, separados por vírgula")
    parser.add_argument("--boots", type=int, default=0, help="entregas da imagem por IP (0 = sem limite)")
    args = parser.parse_args()
    allowed = {ip.strip() for ip in args.allow.split(",") if ip.strip()}
    root = os.path.realpath(args.root)
    os.makedirs(root, exist_ok=True)
    server = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    server.bind((args.bind, args.port))
    print(f"TFTP servindo {root} em {args.bind}:{args.port} para {sorted(allowed)}", flush=True)
    while True:
        request, addr = server.recvfrom(2048)
        # Quem não está em --allow (ou não pede leitura) é descartado em silêncio, sem thread nem resposta:
        # responder a origem forjada serviria de reflexão contra a vítima.
        if addr[0] not in allowed or len(request) < 4 or struct.unpack("!H", request[:2])[0] != RRQ:
            continue
        if not SLOTS.acquire(blocking=False):
            continue  # muitas transferências em curso; o u-boot repete o pedido
        threading.Thread(target=guarded, args=(request, addr, root, allowed, args.boots), daemon=True).start()


if __name__ == "__main__":
    main()
