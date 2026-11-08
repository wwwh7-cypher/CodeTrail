#pragma once

#include "service.h"

namespace app {

class Worker {
public:
    int run(int value);
    int current() const;
};

}  // namespace app
