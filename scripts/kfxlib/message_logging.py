import logging
import threading

__license__ = "GPL v3"
__copyright__ = "2016-2025, John Howell <jhowell@acm.org>"


thread_local_cfg = threading.local()


def set_logger(logger=None):

    if logger is not None:
        thread_local_cfg.logger = logger
    elif hasattr(thread_local_cfg, "logger"):
        del thread_local_cfg.logger

    return logger


def get_current_logger():
    return getattr(thread_local_cfg, "logger", logging)


class LogCurrent(object):

    def __getattr__(self, method_name):
        return getattr(get_current_logger(), method_name)


class JobLog(object):
    '''
    Logger that also collects errors and warnings for presentation in a job summary.
    '''

    def __init__(self, logger):
        self.logger = logger
        self.errors = []
        self.warnings = []

    def debug(self, msg):
        self.logger.debug(msg)

    def info(self, msg):
        self.logger.info(msg)

    def warn(self, msg):
        self.warnings.append(msg)
        self.logger.warn("WARNING: %s" % msg)

    def warning(self, desc):
        self.warn(desc)

    def error(self, msg):
        self.errors.append(msg)
        self.logger.error("ERROR: %s" % msg)

    def exception(self, msg):
        self.errors.append("EXCEPTION: %s" % msg)
        self.logger.exception("EXCEPTION: %s" % msg)

    def __call__(self, *args):
        self.info(" ".join([str(arg) for arg in args]))


log = LogCurrent()
