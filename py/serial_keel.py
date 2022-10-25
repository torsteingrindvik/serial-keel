
import json

from websockets import WebSocketClientProtocol

class SerialKeel():
    ws: WebSocketClientProtocol = None

    def __init__(self, ws: WebSocketClientProtocol) -> None:
        self.ws = ws

    async def observe_mock(self, mock: str):
        # GOAL:
        #
        # "{\"Observe\":{\"Mock\":\"example\"}}"
        msg = json.dumps({'Observe': {'Mock': mock}})

        await self.ws.send(msg)
        response = await self.ws.recv()
        response = json.loads(response)

        print(response)

    async def close(self):
        print('closing')
        await self.ws.write_close_frame("")
