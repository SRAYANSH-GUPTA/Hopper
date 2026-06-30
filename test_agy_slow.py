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

os.write(master, b"/")
print("Typed /, waiting...")
time.sleep(1.5)
print(read_all().decode('utf-8', 'replace'))

for char in "usage":
    os.write(master, char.encode())
    time.sleep(0.1)

print("Typed usage, waiting...")
time.sleep(1)
print(read_all().decode('utf-8', 'replace'))

os.write(master, b"\r")
print("Sent first \\r, waiting...")
time.sleep(1)
print(read_all().decode('utf-8', 'replace'))

os.write(master, b"\r")
print("Sent second \\r, waiting...")
time.sleep(2)
out = read_all()
print("After second \\r:", out.decode('utf-8', 'replace'))

p.terminate()
