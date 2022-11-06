# import json
# import logging
# from pathlib import Path
# import re
# import pytest
# from typing import Dict

# from serial_keel import SerialKeel, Endpoint, connect, logger


# async def tfm_test(sk: SerialKeel, endpoint: Endpoint, spec: Dict):
#     expected_test_cases = set(spec['to_find'])
#     allowed_to_fail = set(spec['allowed_to_fail'])
#     end_condition = spec['end_condition']

#     found_passed = set()
#     found_failed = set()

#     async for message in sk.endpoint_messages(endpoint):
#         if end_condition in message:
#             break

#         # A concluding test has the format:
#         #       TEST: TFM_SOME_NAME_1234 - PASSED!
#         # or
#         #       TEST: TFM_SOME_NAME_1234 - FAILED!
#         pattern = 'TEST: (.*) - (PASSED|FAILED)!'
#         if match := re.search(pattern, message):
#             test_name = match.group(1)
#             verdict = match.group(2)

#             expected_test_cases.remove(test_name)
#             if verdict == 'PASSED':
#                 found_passed.add(test_name)
#             else:
#                 found_failed.add(test_name)

#     if len(found_failed) != 0:
#         for failed in list(found_failed):
#             if failed in allowed_to_fail:
#                 logging.warning(f'Failed but allowed to: {failed}')
#             else:
#                 logging.error(f'Failed: {failed}')
#                 raise RuntimeError('Not all tests passed')

#     if len(expected_test_cases) != 0:
#         for missing in list(expected_test_cases):
#             logging.error(f'Not found: {missing}')
#         raise RuntimeError('Not all tests were executed')


# @pytest.mark.asyncio_cooperative
# async def test_tfm_regression():
#     h = logging.FileHandler(f'log.log', mode='w')
#     h.setFormatter(logging.Formatter(
#         '%(asctime)s [%(levelname)s] %(message)s'))
#     h.setLevel(logging.DEBUG)
#     logger.setLevel(logging.DEBUG)
#     logger.addHandler(h)

#     async with connect("ws://127.0.0.1:3000/ws") as sk:
#         logger.info("Connected")
#         secure_endpoint = await sk.control_mock('mock-tfm-secure', Path('mock/tfm-regression-secure.txt'))
#         non_secure_endpoint = await sk.control_mock('mock-tfm-non-secure', Path('mock/tfm-regression-non-secure.txt'))

#         with open(Path(__file__).parent / 'tfm-spec.json') as f:
#             spec = json.loads(f.read())

#             await tfm_test(sk, secure_endpoint, spec['secure'])
#             await tfm_test(sk, non_secure_endpoint, spec['non-secure'])
