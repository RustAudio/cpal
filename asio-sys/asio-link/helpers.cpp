#include "helpers.hpp"
extern "C" void destruct_AsioDrivers(AsioDrivers * a){
	a->~AsioDrivers();
}
