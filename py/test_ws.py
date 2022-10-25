import logging
import pytest
import websockets

from serial_keel import SerialKeel


logging.basicConfig(
    format="%(message)s",
    level=logging.DEBUG,
)


@pytest.mark.asyncio
async def test_observe():
    async with websockets.connect("ws://127.0.0.1:3000/ws") as ws:
        sk = SerialKeel(ws)
        mock = 'some_mock'
        await sk.observe_mock(mock)
        print('done!')
