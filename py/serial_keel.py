
import asyncio
from enum import Enum
import json
from types import TracebackType
from typing import Any, Deque, Dict, List, Optional, Type

from websockets import WebSocketClientProtocol
import websockets

# TODO
Endpoint = str

# TODO
Response = Dict

class SerialKeelWs:
    ws: WebSocketClientProtocol = None

    def __init__(self, ws: WebSocketClientProtocol):
        self.ws = ws

    async def _send(self, message: str):
        await self.ws.send(message)

    async def _receive(self) -> str:
        return await self.ws.recv()

    async def write(self, endpoint: Endpoint, message: str):
        """
        Serialization format:
            {"Write":[{"Mock":"example"},"Hi there"]}
        """
        await self._send(json.dumps({'Write': [endpoint, message]}))

    async def observe(self, endpoint: Endpoint):
        """
        Serialization format:
            {"Observe":{"Mock":"example"}}
        """
        await self._send(json.dumps({'Observe': endpoint}))

    async def read(self) -> Response:
        response = await self._receive()
        response = json.loads(response)
        return response


class Observer:
    # For example:
    #   {'Mock': 'name-of-mock'}
    # or
    #   ['Tty': '/dev/ttyACM0'}
    endpoint: Dict[str, str] = None
    skws: SerialKeelWs = None

    def __init__(self, skws: WebSocketClientProtocol, endpoint: Dict[str, str]):
        self.skws = skws
        self.endpoint = endpoint
    
    async def write(self, message: str):
        await self.skws.write(self.endpoint, message)

Message = str

class MessageType(Enum):
    # Server status answer to request
    CONTROL = 1,

    # Message contains serial data
    SERIAL = 2,

class SerialKeel:
    skws: SerialKeelWs = None
    observers: List[Endpoint]
    responses: Dict[MessageType, "asyncio.Queue[Message]"]
    reader: "asyncio.Task[None]"


    def __init__(self, ws: WebSocketClientProtocol) -> None:
        self.skws = SerialKeelWs(ws)
        self.observers = []
        self.responses = {
            MessageType.CONTROL: asyncio.Queue(),
            MessageType.SERIAL: asyncio.Queue(),
        }

        loop = asyncio.get_event_loop()
        self.reader = loop.create_task(self._read())

    async def _read(self) -> None:
        while True:
            # print('Good morning yall')
            # await asyncio.sleep(1.)
            response = await self.skws.read()

            if 'Ok' in response:
                print(f'Appending response: {response}')
                await self.responses[MessageType.CONTROL].put(response['Ok'])
            else:
                print(f'Omg: {response}')
                raise RuntimeError(f'Response: {response} not handled')

            print(response)

    async def observe_mock(self, name: str) -> Observer:
        endpoint = {'Mock': name}
        await self.skws.observe(endpoint)
        response = await asyncio.wait_for(self.responses[MessageType.CONTROL].get(), timeout=5.0)
        print(f'Control message: {response}')

        self.observers.append(endpoint)

        return Observer(self.skws, endpoint)


class Connect:
    uri: str = None
    ws: WebSocketClientProtocol = None

    def __init__(self, uri: str) -> None:
        self.uri = uri

    async def __aenter__(self) -> SerialKeel:
        self.ws = await websockets.connect(self.uri)
        return SerialKeel(self.ws)

    async def __aexit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        traceback: Optional[TracebackType],
    ) -> None:
        await self.ws.close()

connect = Connect
