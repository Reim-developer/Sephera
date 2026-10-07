#ifndef ACME_HPP
#define ACME_HPP

#include "shape.hpp"
#include <vector>
#include <string>
#include "absent.hpp"

class Shape {
public:
    virtual double area() const;
};

#endif
