
import asyncio
from enum import Enum
import json
import logging
from types import TracebackType
from typing import Any, Deque, Dict, List, Optional, Type

from websockets import WebSocketClientProtocol
import websockets

logger = logging.getLogger(__name__)

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

class SerialKeelIter:
    queue: "asyncio.Queue[Message]"
    timeout: float

    def __init__(self, queue: "asyncio.Queue[Message]", timeout: float = 10.):
        self.queue = queue
        self.timeout = timeout
    
    def __aiter__(self):
        return self

    async def __anext__(self):
        return await asyncio.wait_for(self.queue.get(), self.timeout)

class SerialKeel:
    skws: SerialKeelWs = None
    observers: List[Endpoint]
    responses: Dict[MessageType, "asyncio.Queue[Message]"]
    reader: "asyncio.Task[None]"
    timeout: float


    def __init__(self, ws: WebSocketClientProtocol, timeout: float = 10.) -> None:
        self.skws = SerialKeelWs(ws)
        self.observers = []
        self.responses = {
            MessageType.CONTROL: asyncio.Queue(),
            # TODO: More queues! For inbox
            MessageType.SERIAL: asyncio.Queue(),
        }

        loop = asyncio.get_event_loop()
        self.reader = loop.create_task(self._read())
        self.timeout = timeout

    async def _read(self) -> None:
        while True:
            response = await self.skws.read()

            if 'Ok' in response:
                value = response['Ok']

                if value == 'Ok':
                    logger.debug(f'Appending control response: {value}')
                    await self.responses[MessageType.CONTROL].put(value)
                elif 'Message' in value:
                    logger.debug(f'Appending message response: {value}')
                    await self.responses[MessageType.SERIAL].put(value['Message'])
                else:
                    logger.debug(f'Not handled: {response}')
                    raise RuntimeError(f'Response value: {response} not handled')

            else:
                logger.debug(f'Not handled: {response}')
                raise RuntimeError(f'Response category: {response} not handled')

    async def observe_mock(self, name: str) -> Observer:
        endpoint = {'Mock': name}
        await self.skws.observe(endpoint)
        response = await asyncio.wait_for(self.responses[MessageType.CONTROL].get(), self.timeout)
        logger.debug(f'Control message: {response}')

        self.observers.append(endpoint)

        return Observer(self.skws, endpoint)
    
    async def get_serial(self, timeout: float = 10.) -> Message:
        return await asyncio.wait_for(self.responses[MessageType.SERIAL].get(), timeout)
    
    def __aiter__(self):
        return SerialKeelIter(self.responses[MessageType.SERIAL], self.timeout)
    


class Connect:
    uri: str = None
    ws: WebSocketClientProtocol = None
    sk: SerialKeel = None

    def __init__(self, uri: str) -> None:
        self.uri = uri

    async def __aenter__(self) -> SerialKeel:
        logger.info(f'Connecting to `{self.uri}`')
        self.ws = await websockets.connect(self.uri)
        self.sk = SerialKeel(self.ws)

        return self.sk

    async def __aexit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        traceback: Optional[TracebackType],
    ) -> None:
        self.sk.reader.cancel()
        await self.ws.close()

connect = Connect
