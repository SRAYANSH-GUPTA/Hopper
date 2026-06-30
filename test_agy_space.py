import pty
import os
import time
import subprocess
import select

master, slave = pty.openpty()
env = os.environ.copy()
env['TERM'] = 'xterm-256color'

p = subprocess.Popen(['agy'], stdin=slave, stdout=slave, stderr=slave, env=env)
os.close(slave)

def read_all():
    out = b""
    while True:
        r, _, _ = select.select([master], [], [], 0.5)
        if master in r:
            out += os.read(master, 4096)
        else:
            break
    return out

print("Starting...")
read_all()

for char in "/usage ":
    os.write(master, char.encode())
    time.sleep(0.1)

print("Typed /usage with space, waiting...")
time.sleep(1)
out = read_all()
print("After typing:", out.decode('utf-8', 'replace'))

os.write(master, b"\r")
time.sleep(2)
out = read_all()
print("After \\r:", out.decode('utf-8', 'replace'))

p.terminate()
