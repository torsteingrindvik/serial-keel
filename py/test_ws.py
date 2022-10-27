import logging
from pathlib import Path
import pytest

from serial_keel import connect


logger = logging.getLogger(__name__)
logging.basicConfig(
    format="%(asctime)s     %(message)s",
    level=logging.DEBUG,
)


@pytest.mark.asyncio
async def test_observe_mock():
    async with connect("ws://127.0.0.1:3000/ws") as sk:
        """
        Now we are already connected to the server.
        The server program is called SerialKeel; therefore 'sk'.

        Now we choose if we're running a mock session or not:
        """
        if True:
            observer = await sk.observe_mock('some-mock')
            """
            This is a mock session.
            Instead of writing to a serial port, when writing
            to a mock, the data will be sent back to us
            line by line.

            This means we can inject any text we want and check
            our logic against it.
            """
            this_folder = Path(__file__).parent
            with open(this_folder / 'hex-output.txt') as f:
                await observer.write(f.read())
        else:
            # Not implemented yet
            observer = await sk.observe('/dev/ttyACMx')
        
        """
        From this point on our test logic has no idea if the data coming in
        is from a real serial port or a mocked one.
        """
        async for response in sk:
            logger.info(response)

            if 'PROJECT EXECUTION SUCCESSFUL' in response['message']:
                break

