import websockets
import asyncio
import json
import logging
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from types import TracebackType
from typing import Dict, List, Optional, Type
from websockets import WebSocketClientProtocol

logger = logging.getLogger(__name__)


# TODO
Response = Dict


class EndpointType(Enum):
    MOCK = 1,
    TTY = 2,

    def __eq__(self, __o: object) -> bool:
        if not isinstance(__o, EndpointType):
            return False
        else:
            return self.value == __o.value

    def __hash__(self) -> int:
        return hash(self.value)


@dataclass
class Endpoint:
    """TODO"""
    name: str
    variant: EndpointType

    def tty(name: str):
        return Endpoint(name, EndpointType.TTY)

    def mock(name: str):
        return Endpoint(name, EndpointType.MOCK)

    def __eq__(self, __o: object) -> bool:
        if not isinstance(__o, Endpoint):
            return False
        else:
            return self.name == __o.name and self.variant == __o.variant

    def __hash__(self) -> int:
        return hash((self.name, self.variant))


class SerialKeelJSONEncoder(json.JSONEncoder):
    def default(self, o):
        if isinstance(o, Endpoint):
            if (o.variant == EndpointType.TTY):
                return {'Tty': o.name}
            elif (o.variant == EndpointType.MOCK):
                return {'Mock': o.name}
            else:
                raise ValueError('Unknown endpoint variant')
        return super().default(o)


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
        await self._send(json.dumps({'Write': [endpoint, message]}, cls=SerialKeelJSONEncoder))

    async def observe(self, endpoint: Endpoint):
        """
        Serialization format:
            {"Observe":{"Mock":"example"}}
        """
        await self._send(json.dumps({'Observe': endpoint}, cls=SerialKeelJSONEncoder))

    async def read(self) -> Response:
        response = await self._receive()
        response = json.loads(response)
        return response


Message = str


class MessageType(Enum):
    # Server status answer to request
    CONTROL = 1,

    # Message contains serial data
    SERIAL = 2,


class EndpointMessages:
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
    endpoints: List[Endpoint]
    responses: Dict[MessageType, "asyncio.Queue[Message]"]
    reader: "asyncio.Task[None]"
    timeout: float

    def __init__(self, ws: WebSocketClientProtocol, timeout: float = 10.) -> None:
        self.skws = SerialKeelWs(ws)
        self.responses = {
            MessageType.CONTROL: asyncio.Queue(),
            MessageType.SERIAL: {},
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

                    message = value['Message']
                    endpoint = message['endpoint']

                    if 'Mock' in endpoint:
                        endpoint = Endpoint.mock(endpoint['Mock'])
                    elif 'Tty' in endpoint:
                        endpoint = Endpoint.tty(endpoint['Tty'])
                    else:
                        raise ValueError(
                            f'Unknown endpoint variant: {endpoint}')

                    await self.responses[MessageType.SERIAL][endpoint].put(message['message'])
                else:
                    logger.debug(f'Not handled: {response}')
                    raise RuntimeError(
                        f'Response value: {response} not handled')

            else:
                logger.debug(f'Not handled: {response}')
                raise RuntimeError(
                    f'Response category: {response} not handled')

    async def observe(self, tty: str) -> Endpoint:
        """
        Start observing ("subscribing") to an endpoint.

        The tty's lines will be received line by line.
        """
        raise NotImplementedError

    async def observe_mock(self, name: str, file: Path) -> Endpoint:
        """
        Start observing ("subscribing") to a mock endpoint.

        The mock file's contents will be received line by line.
        """
        endpoint = Endpoint.mock(name)

        await self.skws.observe(endpoint)
        response = await asyncio.wait_for(self.responses[MessageType.CONTROL].get(), self.timeout)
        logger.debug(f'Control message: {response}')

        self.responses[MessageType.SERIAL][endpoint] = asyncio.Queue()

        this_folder = Path(__file__).parent
        with open(this_folder / file) as f:
            await self.write(endpoint, f.read())

        return endpoint

    async def write(self, endpoint: Endpoint, message: str):
        await self.skws.write(endpoint, message)
        response = await asyncio.wait_for(self.responses[MessageType.CONTROL].get(), self.timeout)
        logger.debug(f'Control message: {response}')

    def endpoint_messages(self, endpoint: Endpoint) -> EndpointMessages:
        return EndpointMessages(self.responses[MessageType.SERIAL][endpoint], self.timeout)


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
