import pytest
import asyncio
import logging


@pytest.mark.asyncio
async def test_foo():
    logging.info("hi")
    await asyncio.sleep(1)
    logging.info("bye")
