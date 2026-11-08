#include "worker.hpp"

#include "service.h"

namespace app {

int Worker::run(int value) {
    return service_add(value, 1);
}

int Worker::current() const {
    return 0;
}

}  // namespace app
