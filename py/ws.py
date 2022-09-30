import asyncio
import logging
import time
import websockets

logging.basicConfig(
    format="%(message)s",
    level=logging.DEBUG,
)


async def hello():
    n = 0
    async with websockets.connect("ws://localhost:3000/ws") as websocket:
        try:
            start = time.time()
            await websocket.send("Hello world!")

            async for message in websocket:
                print(f'Got: `{message}` -- {n}')
                n += 1
                await websocket.send("Hello world!")
                await asyncio.sleep(0.1)
        except Exception as e:
            print(f'Oh no: {e} after {time.time() - start}')


asyncio.run(hello())
