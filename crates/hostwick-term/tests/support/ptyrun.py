"""Run a command in a pseudo terminal and relay it over stdin/stdout, like
the remote end of an SSH session. Used by the trzsz tests."""
import os
import pty
import select
import sys


def main():
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(sys.argv[1], sys.argv[1:])
    stdin_open = True
    while True:
        fds = [fd] + ([0] if stdin_open else [])
        ready, _, _ = select.select(fds, [], [])
        if fd in ready:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            os.write(1, data)
        if 0 in ready:
            data = os.read(0, 65536)
            if not data:
                stdin_open = False
            else:
                os.write(fd, data)
    _, status = os.waitpid(pid, 0)
    sys.exit(os.waitstatus_to_exitcode(status))


main()
