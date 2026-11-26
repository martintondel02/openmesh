#!/usr/bin/env python3
"""OpenMesh two-node RF bring-up harness (companion protocol over USB CDC).

Usage: two_node_test.py /dev/ttyACM0 [/dev/ttyACM1]
Sends Hello/Stats to each radio, then injects a golden OMP frame on node A and checks that node B's
rx_frames counter increases. This proves both radios are on the same channel/profile and that TX/RX
works end to end, without needing app-layer keys.
"""
import cbor2, serial, struct, sys, time

MAGIC=b'OM'; T_HELLO=1; T_TX=6; T_STATS_GET=8; T_PING=10; T_STATS=9; T_HELLO_ACK=2; T_PONG=11
FRAME=bytes.fromhex('208064a0f52274a2ccb41d7bf3da583cced411223344ffffffffffffffff00000002')

def crc16(data):
    crc=0xFFFF
    for b in data:
        crc ^= b<<8
        for _ in range(8):
            crc=((crc<<1)^0x1021)&0xFFFF if crc&0x8000 else (crc<<1)&0xFFFF
    return crc

def slip(data):
    out=bytearray([0xC0])
    for b in data:
        if b==0xC0: out+=bytes([0xDB,0xDC])
        elif b==0xDB: out+=bytes([0xDB,0xDD])
        else: out.append(b)
    out.append(0xC0); return bytes(out)

def chunk(t,payload):
    body=bytes([t])+struct.pack('<H',len(payload))+payload
    return b'OM'+body+struct.pack('<H',crc16(body))

def unescape(data):
    out=bytearray(); i=0
    while i<len(data):
        if data[i]==0xDB and i+1<len(data):
            out.append(0xDC if data[i+1]==0xDC else 0xDD if data[i+1]==0xDD else data[i+1]); i+=2
        else: out.append(data[i]); i+=1
    return bytes(out)

class Radio:
    def __init__(self,path):
        # write_timeout keeps a broken/hung CDC endpoint from blocking the whole harness.
        self.ser=serial.Serial(path,115200,timeout=0.05,write_timeout=1.0)
        self.buf=bytearray()
    def send(self,t,payload):
        if not isinstance(payload,(bytes,bytearray)): payload=cbor2.dumps(payload)
        self.ser.write(slip(chunk(t,payload))); self.ser.flush()
    def frames(self,duration=1.0):
        out=[]; end=time.time()+duration
        while time.time()<end:
            b=self.ser.read(1)
            if not b: continue
            if b==b'\xC0':
                if len(self.buf)>2:
                    raw=unescape(bytes(self.buf))
                    if raw[:2]==MAGIC and len(raw)>=7:
                        t=raw[2]; ln=struct.unpack('<H',raw[3:5])[0]
                        try: out.append((t,cbor2.loads(raw[5:5+ln])))
                        except Exception: pass
                self.buf=bytearray()
            else: self.buf.extend(b)
        return out
    def request(self,t,payload,duration=1.5):
        self.send(t,payload); return self.frames(duration)
    def hello(self):
        for t,m in self.request(T_HELLO,{"proto":1,"app_version":"qa","wants_events":False}):
            if t==T_HELLO_ACK: return m
        return None
    def stats(self):
        for t,m in self.request(T_STATS_GET,{}):
            if t==T_STATS:
                if isinstance(m,dict) and 'entries' in m and isinstance(m['entries'],list):
                    out={}
                    for e in m['entries']:
                        if isinstance(e,(list,tuple)) and len(e)==2: out[str(e[0])]=e[1]
                        elif isinstance(e,dict): out[e.get('key')]=e.get('value')
                    return out
                if isinstance(m,dict): return m
                return m
        return None
    def ping(self,nonce):
        for t,m in self.request(T_PING,{"nonce":nonce}):
            if t==T_PONG: return m.get('nonce')==nonce
        return False

def main():
    paths=sys.argv[1:] or ['/dev/ttyACM0','/dev/ttyACM1']
    radios=[]
    for p in paths:
        try:
            r=Radio(p); radios.append((p,r))
        except Exception as e:
            print('open',p,'failed',e)
    if not radios:
        sys.exit('no radios opened')
    for p,r in radios:
        print(p,'ping',r.ping(0xA5A50000+len(radios)))
        print(p,'hello',r.hello())
        print(p,'stats',r.stats())
    if len(radios)<2: return
    (pa,ra),(pb,rb)=radios[0],radios[1]
    sa0=ra.stats() or {}; sb0=rb.stats() or {}
    print('A->B inject',FRAME.hex())
    ra.send(T_TX,{"frame":FRAME})
    time.sleep(2.0)
    sa1=ra.stats() or {}; sb1=rb.stats() or {}
    print('A tx delta',sa1.get('tx_frames'),'->',sa0.get('tx_frames'), ' airtime',sa1.get('airtime_ms'))
    print('B rx delta',sb1.get('rx_frames'),'->',sb0.get('rx_frames'))
    rx_ok=int(sb1.get('rx_frames',0))>int(sb0.get('rx_frames',0))
    print('RF A->B', 'PASS' if rx_ok else 'FAIL (check antennas/profile/region)')
    # reverse direction
    sb0=sb1; sa0=sa1
    rb.send(T_TX,{"frame":FRAME})
    time.sleep(2.0)
    sa1=ra.stats() or {}; sb1=rb.stats() or {}
    rev_ok=int(sa1.get('rx_frames',0))>int(sa0.get('rx_frames',0))
    print('RF B->A', 'PASS' if rev_ok else 'FAIL')
    sys.exit(0 if (rx_ok and rev_ok) else 2)

if __name__=='__main__': main()
