#!/usr/bin/env python3
"""Server di Tiny Swords: file statici del gioco + relay WebSocket per il multigiocatore.

Uso:  python3 server.py [porta] [--bind INDIRIZZO]      (predefiniti: 8000 e 0.0.0.0)

Solo libreria standard. La simulazione gira nel browser dell'host (il primo che entra nella
lobby); il server si limita a inoltrare messaggi di testo:
    client -> host    arriva all'host come "<id client>|<messaggio>"
    host -> client    "<id client>|<messaggio>", oppure "*|<messaggio>" per tutti
    server -> host    "0|<messaggio>" (un client se n'è andato)
"""
import argparse
import base64
import functools
import hashlib
import json
import os
import queue
import socket
import struct
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.dirname(os.path.abspath(__file__))
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
MAX_MESSAGE = 4 << 20  # byte
IDLE_TIMEOUT = 90      # secondi senza traffico prima di considerare morta una connessione
BACKLOG = 400          # messaggi in coda verso un peer lento prima di scollegarlo


def frame(payload, opcode=1):
    n = len(payload)
    if n < 126:
        head = bytes([0x80 | opcode, n])
    elif n < 65536:
        head = bytes([0x80 | opcode, 126]) + struct.pack(">H", n)
    else:
        head = bytes([0x80 | opcode, 127]) + struct.pack(">Q", n)
    return head + payload


class Peer:
    """Una connessione WebSocket. Le scritture passano da una coda e da un thread dedicato,
    così un peer bloccato non ferma chi gli sta inviando dati."""

    def __init__(self, sock, pid):
        self.sock, self.id = sock, pid
        self.out = queue.Queue(BACKLOG)
        threading.Thread(target=self._write, daemon=True).start()

    def send(self, payload, opcode=1):
        try:
            self.out.put_nowait(frame(payload, opcode))
        except queue.Full:
            self.drop()

    def close(self):
        """Chiude dopo aver spedito ciò che è in coda."""
        try:
            self.out.put_nowait(None)
        except queue.Full:
            self.drop()

    def drop(self):
        try:
            self.sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

    def _write(self):
        while True:
            data = self.out.get()
            if data is None:
                break
            try:
                self.sock.sendall(data)
            except OSError:
                break
        self.drop()


class Room:
    """L'unica partita del server: un host e i suoi client."""

    def __init__(self):
        self.lock = threading.Lock()
        self.host = None
        self.clients = {}
        self.next_id = 1

    def join(self, sock):
        with self.lock:
            peer = Peer(sock, self.next_id)
            self.next_id += 1
            is_host = self.host is None
            if is_host:
                self.host = peer
            else:
                self.clients[peer.id] = peer
        return peer, is_host

    def leave(self, peer):
        orphans, host = [], None
        with self.lock:
            if self.host is peer:  # senza host la partita è finita: via tutti, il prossimo sarà il nuovo host
                self.host = None
                orphans, self.clients = list(self.clients.values()), {}
            elif self.clients.pop(peer.id, None):
                host = self.host
        for p in orphans:
            p.send(b'{"t":"hostleft"}')
            p.close()
        if host:
            host.send(b'0|{"t":"leave","id":%d}' % peer.id)
        peer.close()

    def route(self, peer, msg):
        if peer is self.host:
            dest, sep, body = msg.partition(b"|")
            if not sep:
                return
            with self.lock:
                if dest == b"*":
                    targets = list(self.clients.values())
                else:
                    target = self.clients.get(int(dest)) if dest.isdigit() else None
                    targets = [target] if target else []
            for p in targets:
                p.send(body)
            return
        with self.lock:
            host = self.host if peer.id in self.clients else None
        if host:
            host.send(b"%d|" % peer.id + msg)


class Handler(SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"  # richiesto dall'handshake WebSocket
    room = Room()

    def do_GET(self):
        if self.path.split("?")[0] == "/ws":
            self.websocket()
        else:
            super().do_GET()

    def end_headers(self):
        # il browser rivalida sempre: dopo un aggiornamento host e client caricano la stessa versione
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_request(self, code="-", size="-"):
        pass  # niente riga per ogni file servito; gli errori restano visibili

    def websocket(self):
        key = self.headers.get("Sec-WebSocket-Key")
        origin = self.headers.get("Origin")
        if self.headers.get("Upgrade", "").lower() != "websocket" or not key:
            self.send_error(400, "WebSocket richiesto")
            return
        if origin and origin.split("://", 1)[-1] != self.headers.get("Host"):
            self.send_error(403, "Origine non ammessa")  # solo le pagine servite da questo server
            return
        accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        self.send_response(101, "Switching Protocols")
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.close_connection = True
        self.connection.settimeout(IDLE_TIMEOUT)

        peer, is_host = self.room.join(self.connection)
        who = "host" if is_host else "giocatore"
        print(f"+ {who} {peer.id} da {self.client_address[0]}", flush=True)
        hello = {"t": "hello", "id": peer.id, "host": is_host, "urls": self.server.urls}
        peer.send(json.dumps(hello).encode())
        try:
            for msg in self.messages(peer):
                if msg:  # il messaggio vuoto è solo un segnale di vita
                    self.room.route(peer, msg)
        except (OSError, EOFError, ValueError):
            pass
        finally:
            self.room.leave(peer)
            print(f"- {who} {peer.id}", flush=True)

    def messages(self, peer):
        """Genera i messaggi completi ricevuti dal browser, fino alla chiusura."""

        def need(n):
            data = self.rfile.read(n)
            if len(data) < n:
                raise EOFError
            return data

        buf = b""
        while True:
            b0, b1 = need(2)
            opcode, n = b0 & 15, b1 & 127
            if n == 126:
                (n,) = struct.unpack(">H", need(2))
            elif n == 127:
                (n,) = struct.unpack(">Q", need(8))
            if not b1 & 128 or len(buf) + n > MAX_MESSAGE:
                raise ValueError("frame non valido")
            mask, data = need(4), need(n)
            if n:
                key = int.from_bytes((mask * (n // 4 + 1))[:n], "big")
                data = (int.from_bytes(data, "big") ^ key).to_bytes(n, "big")
            if opcode == 8:
                return
            if opcode == 9:
                peer.send(data, 10)
            elif opcode in (0, 1, 2):
                buf += data
                if b0 & 128:
                    yield buf
                    buf = b""


def lan_urls(bind, port):
    """Indirizzi da dare agli altri giocatori; per primo quello dell'interfaccia di rete principale."""
    if bind not in ("", "0.0.0.0"):
        return [f"http://{bind}:{port}"]
    ips = []
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.connect(("10.255.255.255", 1))  # non invia nulla: serve a scoprire l'interfaccia d'uscita
            ips.append(s.getsockname()[0])
    except OSError:
        pass
    try:
        ips += sorted(set(socket.gethostbyname_ex(socket.gethostname())[2]) - set(ips))
    except OSError:
        pass
    return [f"http://{ip}:{port}" for ip in ips if not ip.startswith("127.")]


def main():
    ap = argparse.ArgumentParser(description="Tiny Swords: gioco e relay per il multigiocatore")
    ap.add_argument("port", nargs="?", type=int, default=8000)
    ap.add_argument("--bind", "-b", default="0.0.0.0", metavar="INDIRIZZO")
    args = ap.parse_args()

    server = ThreadingHTTPServer((args.bind, args.port), functools.partial(Handler, directory=ROOT))
    server.urls = lan_urls(args.bind, args.port)
    print(f"Tiny Swords su http://localhost:{args.port}  (Ctrl+C per fermare)")
    for url in server.urls:
        print(f"Gli altri giocatori aprono: {url}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print()


if __name__ == "__main__":
    main()
