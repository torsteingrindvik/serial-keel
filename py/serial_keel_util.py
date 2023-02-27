import logging
from pathlib import Path


def make_logger(name: str, add_formatter: bool = False) -> logging.Logger:
    logger = logging.getLogger(f'{name}')

    logdir = Path('logs')
    logdir.mkdir(parents=True, exist_ok=True)

    logfile = logdir / f'{name}.log'

    h = logging.FileHandler(logfile, mode='w')
    if add_formatter:
        h.setFormatter(logging.Formatter('%(asctime)s [%(levelname)s] %(message)s'))
    h.setLevel(logging.DEBUG)
    logger.setLevel(logging.DEBUG)

    # Note that depending on how many tests parameterized,
    # this almost doubles time executed
    logger.addHandler(h)  # <--

    return logger
