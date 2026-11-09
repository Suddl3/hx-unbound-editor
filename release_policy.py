"""Public preview boundaries. No device discovery or I/O in this module."""
BLOCKED_OPERATIONS = frozenset({'nam', 'nam-install', 'ir-install', 'inspect-slot'})
TRANSFER_MESSAGE = ('Device capture/IR transfers and verified NAM selection are unavailable '
                    'in this preview while a panel-freeze issue is investigated. '
                    'Local NAM importing and browsing remain available.')
DEVICE_MESSAGE = 'Device access is disabled. Relaunch with -ExperimentalDevice for opt-in hardware testing.'

def capabilities(enabled=False):
    return {'app': 'HX Unbound', 'preview': True, 'deviceEnabled': bool(enabled),
            'transfersEnabled': False, 'transferMessage': TRANSFER_MESSAGE,
            'deviceMessage': DEVICE_MESSAGE}

def require_edit(enabled, command):
    if not enabled:
        raise ValueError(DEVICE_MESSAGE)
    if command.get('op') in BLOCKED_OPERATIONS:
        raise ValueError(TRANSFER_MESSAGE)
