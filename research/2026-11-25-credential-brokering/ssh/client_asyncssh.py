"""asyncssh logs in through the boundary with the agent's key only."""
import asyncio, sys, asyncssh
async def main():
    async with asyncssh.connect("127.0.0.1", int(sys.argv[2]), username="deploy", agent_path=sys.argv[1],
                                known_hosts=None) as c:
        print((await c.run("echo ok")).stdout.strip())
asyncio.run(main())
