def run_job(raw):
    """后台任务入口：预处理任务数据并归档。"""
    record = decode_record(raw)
    return archive_record(record)


def decode_record(raw):
    """解码任务内容。"""
    return raw.strip()


def archive_record(record):
    """为已处理记录添加归档标记。"""
    return {"archived": True, "record": record}
