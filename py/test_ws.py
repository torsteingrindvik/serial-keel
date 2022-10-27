import asyncio
import logging
import pytest

from serial_keel import connect


logging.basicConfig(
    format="%(asctime)s     %(message)s",
    level=logging.DEBUG,
)


@pytest.mark.asyncio
async def test_observe():
    async with connect("ws://127.0.0.1:3000/ws") as sk:
        observer = await sk.observe_mock('some-mock')

        await observer.write('It is hi')

        # response = await observer.read()
        # print(f'Got: {response}')

        # response = await observer.read()
        # print(f'Got: {response}')

        # await asyncio.sleep(5.)
